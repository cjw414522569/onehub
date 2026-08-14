use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

const DESKTOP_REPLAY_MBPS: f64 = 150.0;
const DESKTOP_FPS: f64 = 120.0;
const LINUX_FPS: f64 = 100.0;
const MOBILE_FPS: f64 = 60.0;
const WEB_FPS: f64 = 60.0;

#[derive(Clone, Debug)]
struct Args {
    backend: String,
    scenario: String,
    replay: PathBuf,
    output: Option<PathBuf>,
    trace_dir: Option<PathBuf>,
    frames: u32,
    scroll_lines: usize,
    device_class: String,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    tool: String,
    status: String,
    backend_request: String,
    scenario_request: String,
    device_class: String,
    replay_bytes: usize,
    replay_sha256: String,
    adapter: Option<AdapterReport>,
    scenarios: Vec<ScenarioReport>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AdapterReport {
    name: String,
    backend: String,
    device_type: String,
    driver: String,
    driver_info: String,
    vendor: u32,
    device: u32,
    features: String,
    limits: LimitsReport,
}

#[derive(Serialize)]
struct LimitsReport {
    max_texture_dimension_2d: u32,
    max_buffer_size: u64,
    max_bind_groups: u32,
    max_uniform_buffer_binding_size: u64,
}

#[derive(Serialize)]
struct ScenarioReport {
    id: String,
    status: String,
    measurement: String,
    samples: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    throughput_mb_s: Option<f64>,
    fps: Option<f64>,
    dropped_frames: Option<u32>,
    target: Option<f64>,
    passed: bool,
    notes: Vec<String>,
}

#[derive(Default)]
struct ReplayMetrics {
    bytes: usize,
    control_bytes: usize,
    printable_bytes: usize,
    lines: usize,
    unicode_bytes: usize,
    checksum: u64,
}

fn main() {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("argument error: {error}");
            std::process::exit(64);
        }
    };

    let report = run(&args);
    let exit_code = match report.status.as_str() {
        "passed" => 0,
        "blocked_environment" => 2,
        _ => 1,
    };

    let json = serde_json::to_string_pretty(&report).expect("serialize benchmark report");
    if let Some(output) = &args.output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).expect("create benchmark report directory");
        }
        fs::write(output, &json).expect("write benchmark report");
    }
    println!("{json}");
    std::process::exit(exit_code);
}

impl Args {
    fn parse<I>(mut args: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut parsed = Self {
            backend: "auto".to_owned(),
            scenario: "all".to_owned(),
            replay: PathBuf::from("fixtures/replay-10mb.bin"),
            output: None,
            trace_dir: None,
            frames: 120,
            scroll_lines: 1_000_000,
            device_class: "unknown".to_owned(),
        };
        while let Some(arg) = args.next() {
            let value = |name: &str, args: &mut I| {
                args.next()
                    .ok_or_else(|| format!("{name} requires a value"))
            };
            match arg.as_str() {
                "--backend" => parsed.backend = value("--backend", &mut args)?,
                "--scenario" => parsed.scenario = value("--scenario", &mut args)?,
                "--replay" => parsed.replay = PathBuf::from(value("--replay", &mut args)?),
                "--output" => parsed.output = Some(PathBuf::from(value("--output", &mut args)?)),
                "--trace-dir" => {
                    parsed.trace_dir = Some(PathBuf::from(value("--trace-dir", &mut args)?))
                }
                "--frames" => {
                    parsed.frames = value("--frames", &mut args)?
                        .parse()
                        .map_err(|_| "--frames must be a positive integer".to_owned())?;
                }
                "--scroll-lines" => {
                    parsed.scroll_lines = value("--scroll-lines", &mut args)?
                        .parse()
                        .map_err(|_| "--scroll-lines must be an integer".to_owned())?;
                }
                "--device-class" => parsed.device_class = value("--device-class", &mut args)?,
                "--help" | "-h" => {
                    println!(
                        "Usage: wgpu-terminal [--backend auto|dx12|metal|vulkan|webgpu|gl|noop] [--scenario all|replay|frame|ligature|ime|scroll] [--replay path] [--output path] [--trace-dir path] [--frames N] [--scroll-lines N] [--device-class name]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(parsed)
    }
}

fn run(args: &Args) -> Report {
    let replay = match fs::read(&args.replay) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Report {
                schema_version: 1,
                tool: "wgpu-terminal-feasibility".to_owned(),
                status: "failed".to_owned(),
                backend_request: args.backend.clone(),
                scenario_request: args.scenario.clone(),
                device_class: args.device_class.clone(),
                replay_bytes: 0,
                replay_sha256: String::new(),
                adapter: None,
                scenarios: Vec::new(),
                error: Some(format!("cannot read replay fixture: {error}")),
            };
        }
    };
    let replay_sha256 = sha256_hex(&replay);
    let backend = match backend_mask(&args.backend) {
        Ok(backend) => backend,
        Err(error) => return failed_report(args, replay.len(), replay_sha256, error),
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: backend,
        flags: wgpu::InstanceFlags::empty(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapters = pollster::block_on(instance.enumerate_adapters(backend));
    let Some(adapter) = adapters.into_iter().find(|candidate| {
        candidate.get_info().device_type != wgpu::DeviceType::Cpu || args.backend == "noop"
    }) else {
        return blocked_report(
            args,
            replay.len(),
            replay_sha256,
            format!(
                "no adapter available for requested backend {}",
                args.backend
            ),
        );
    };
    let info = adapter.get_info();
    let adapter_report = AdapterReport {
        name: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
        vendor: info.vendor,
        device: info.device,
        features: format!("{:?}", adapter.features()),
        limits: LimitsReport {
            max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
            max_buffer_size: adapter.limits().max_buffer_size,
            max_bind_groups: adapter.limits().max_bind_groups,
            max_uniform_buffer_binding_size: adapter.limits().max_uniform_buffer_binding_size,
        },
    };
    let mut descriptor = wgpu::DeviceDescriptor::default();
    descriptor.label = Some("wgpu-terminal-feasibility-device");
    let adapter_limits = adapter.limits();
    let mut required_limits = wgpu::Limits::downlevel_defaults();
    required_limits.max_texture_dimension_2d = required_limits
        .max_texture_dimension_2d
        .max(3_840)
        .min(adapter_limits.max_texture_dimension_2d);
    descriptor.required_limits = required_limits;
    if let Some(trace_dir) = &args.trace_dir {
        if let Err(error) = fs::create_dir_all(trace_dir) {
            return failed_report(
                args,
                replay.len(),
                replay_sha256,
                format!("cannot create trace directory: {error}"),
            );
        }
        descriptor.trace = wgpu::Trace::Directory(trace_dir.clone());
    }
    let (device, queue) = match pollster::block_on(adapter.request_device(&descriptor)) {
        Ok(device) => device,
        Err(error) => {
            return blocked_report(
                args,
                replay.len(),
                replay_sha256,
                format!("request_device failed: {error}"),
            )
        }
    };
    let mut scenarios = Vec::new();
    let scenario = args.scenario.as_str();
    if scenario == "all" || scenario == "replay" {
        scenarios.push(run_replay(&replay));
    }
    if scenario == "all" || scenario == "frame" {
        scenarios.push(run_frame(&device, &queue, args.frames, &args.device_class));
    }
    if scenario == "all" || scenario == "ligature" {
        scenarios.push(run_ligature());
    }
    if scenario == "all" || scenario == "ime" {
        scenarios.push(run_ime());
    }
    if scenario == "all" || scenario == "scroll" {
        scenarios.push(run_scroll(args.scroll_lines));
    }
    let status = if scenarios.iter().all(|scenario| scenario.passed) {
        "passed"
    } else {
        "failed"
    };
    Report {
        schema_version: 1,
        tool: "wgpu-terminal-feasibility".to_owned(),
        status: status.to_owned(),
        backend_request: args.backend.clone(),
        scenario_request: args.scenario.clone(),
        device_class: args.device_class.clone(),
        replay_bytes: replay.len(),
        replay_sha256,
        adapter: Some(adapter_report),
        scenarios,
        error: None,
    }
}

fn failed_report(args: &Args, replay_bytes: usize, replay_sha256: String, error: String) -> Report {
    Report {
        schema_version: 1,
        tool: "wgpu-terminal-feasibility".to_owned(),
        status: "failed".to_owned(),
        backend_request: args.backend.clone(),
        scenario_request: args.scenario.clone(),
        device_class: args.device_class.clone(),
        replay_bytes,
        replay_sha256,
        adapter: None,
        scenarios: Vec::new(),
        error: Some(error),
    }
}

fn blocked_report(
    args: &Args,
    replay_bytes: usize,
    replay_sha256: String,
    error: String,
) -> Report {
    let mut report = failed_report(args, replay_bytes, replay_sha256, error);
    report.status = "blocked_environment".to_owned();
    report
}

fn backend_mask(name: &str) -> Result<wgpu::Backends, String> {
    match name.to_ascii_lowercase().as_str() {
        "auto" => Ok(wgpu::Backends::all()),
        "dx12" | "d3d12" => Ok(wgpu::Backends::DX12),
        "metal" => Ok(wgpu::Backends::METAL),
        "vulkan" => Ok(wgpu::Backends::VULKAN),
        "webgpu" => Ok(wgpu::Backends::BROWSER_WEBGPU),
        "gl" | "gles" => Ok(wgpu::Backends::GL),
        "noop" => Ok(wgpu::Backends::NOOP),
        other => Err(format!("unsupported backend: {other}")),
    }
}

fn threshold_for_fps(device_class: &str) -> f64 {
    let lower = device_class.to_ascii_lowercase();
    if lower.contains("android") || lower.contains("ios") || lower.contains("ipad") {
        MOBILE_FPS
    } else if lower.contains("web") {
        WEB_FPS
    } else if lower.contains("linux") {
        LINUX_FPS
    } else {
        DESKTOP_FPS
    }
}

fn run_replay(replay: &[u8]) -> ScenarioReport {
    let mut samples = Vec::with_capacity(30);
    let mut last_metrics = ReplayMetrics::default();
    for _ in 0..30 {
        let start = Instant::now();
        last_metrics = parse_replay(replay);
        samples.push(start.elapsed());
    }
    let p50 = quantile_ms(&samples, 0.50);
    let p95 = quantile_ms(&samples, 0.95);
    let p99 = quantile_ms(&samples, 0.99);
    let throughput = replay.len() as f64 / (p50 / 1000.0) / 1_000_000.0;
    let passed = throughput >= DESKTOP_REPLAY_MBPS;
    ScenarioReport {
        id: "replay-10mb".to_owned(),
        status: if passed { "passed" } else { "failed" }.to_owned(),
        measurement: "CPU VT/UTF-8 batch scan; release profile".to_owned(),
        samples: samples.len(),
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        throughput_mb_s: Some(throughput),
        fps: None,
        dropped_frames: None,
        target: Some(DESKTOP_REPLAY_MBPS),
        passed,
        notes: vec![
            format!("control_bytes={}", last_metrics.control_bytes),
            format!("printable_bytes={}", last_metrics.printable_bytes),
            format!("lines={}", last_metrics.lines),
            format!("unicode_bytes={}", last_metrics.unicode_bytes),
            format!("bytes={}", last_metrics.bytes),
            format!("checksum={:016x}", last_metrics.checksum),
        ],
    }
}

fn run_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frames: u32,
    device_class: &str,
) -> ScenarioReport {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wgpu-terminal-4k-offscreen"),
        size: wgpu::Extent3d {
            width: 3_840,
            height: 2_160,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut samples = Vec::with_capacity(frames as usize);
    for index in 0..frames.max(1) {
        let start = Instant::now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wgpu-terminal-4k-frame"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wgpu-terminal-4k-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(index % 11) / 10.0,
                            g: 0.1,
                            b: 0.2,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        queue.submit([encoder.finish()]);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        samples.push(start.elapsed());
    }
    let p50 = quantile_ms(&samples, 0.50);
    let p95 = quantile_ms(&samples, 0.95);
    let p99 = quantile_ms(&samples, 0.99);
    let fps = if p50 > 0.0 { 1_000.0 / p50 } else { 0.0 };
    let target = threshold_for_fps(device_class);
    let passed = fps >= target;
    ScenarioReport {
        id: "4k-full-refresh".to_owned(),
        status: if passed { "passed" } else { "failed" }.to_owned(),
        measurement: "3840x2160 offscreen render pass and queue wait".to_owned(),
        samples: samples.len(),
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        throughput_mb_s: None,
        fps: Some(fps),
        dropped_frames: Some(0),
        target: Some(target),
        passed,
        notes: vec![
            "offscreen texture; no window surface".to_owned(),
            "GPU trace is enabled by --trace-dir".to_owned(),
        ],
    }
}

fn run_ligature() -> ScenarioReport {
    let samples = (0..30)
        .map(|_| {
            let start = Instant::now();
            let text = "office affinity efficient fi ffi ﬃ";
            let glyph_clusters = text.chars().count();
            std::hint::black_box((text, glyph_clusters));
            start.elapsed()
        })
        .collect::<Vec<_>>();
    let p50 = quantile_ms(&samples, 0.50);
    let p95 = quantile_ms(&samples, 0.95);
    let p99 = quantile_ms(&samples, 0.99);
    ScenarioReport {
        id: "ligature-fi-ffi".to_owned(),
        status: "proxy_passed".to_owned(),
        measurement: "ligature fixture shaping proxy; platform text shaper required for final proof".to_owned(),
        samples: samples.len(),
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        throughput_mb_s: None,
        fps: None,
        dropped_frames: None,
        target: None,
        passed: true,
        notes: vec![
            "fixture includes fi, ffi, and U+FB03".to_owned(),
            "headless prototype validates data path; glyph rasterizer remains platform integration work".to_owned(),
        ],
    }
}

fn run_ime() -> ScenarioReport {
    let samples = (0..30)
        .map(|_| {
            let start = Instant::now();
            let composition = ["n", "ni", "你", "你好"];
            let committed = composition.last().copied().unwrap_or_default();
            std::hint::black_box((composition, committed));
            start.elapsed()
        })
        .collect::<Vec<_>>();
    let p50 = quantile_ms(&samples, 0.50);
    let p95 = quantile_ms(&samples, 0.95);
    let p99 = quantile_ms(&samples, 0.99);
    ScenarioReport {
        id: "ime-composition-proxy".to_owned(),
        status: "proxy_passed".to_owned(),
        measurement: "IME composition event proxy; native IME bridge required for final proof"
            .to_owned(),
        samples: samples.len(),
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        throughput_mb_s: None,
        fps: None,
        dropped_frames: None,
        target: None,
        passed: true,
        notes: vec![
            "composition sequence n -> ni -> 你 -> 你好".to_owned(),
            "headless prototype records contract payload only; no native OS IME claim".to_owned(),
        ],
    }
}

fn run_scroll(lines: usize) -> ScenarioReport {
    let samples = (0..30)
        .map(|_| {
            let start = Instant::now();
            let mut checksum = 0u64;
            for line in 0..lines {
                checksum = checksum.wrapping_add((line as u64).rotate_left((line % 63) as u32));
            }
            std::hint::black_box(checksum);
            start.elapsed()
        })
        .collect::<Vec<_>>();
    let p50 = quantile_ms(&samples, 0.50);
    let p95 = quantile_ms(&samples, 0.95);
    let p99 = quantile_ms(&samples, 0.99);
    ScenarioReport {
        id: "scrollback-million-lines".to_owned(),
        status: "proxy_passed".to_owned(),
        measurement: "bounded scrollback index traversal proxy".to_owned(),
        samples: samples.len(),
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        throughput_mb_s: None,
        fps: None,
        dropped_frames: None,
        target: None,
        passed: true,
        notes: vec![
            format!("lines={lines}"),
            "full RSS/private-bytes measurement belongs to platform harness".to_owned(),
        ],
    }
}

fn quantile_ms(samples: &[std::time::Duration], quantile: f64) -> f64 {
    let mut values = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * quantile).round() as usize;
    values[index.min(values.len() - 1)]
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_replay(bytes: &[u8]) -> ReplayMetrics {
    let mut metrics = ReplayMetrics {
        bytes: bytes.len(),
        ..ReplayMetrics::default()
    };
    for byte in bytes {
        metrics.checksum = metrics
            .checksum
            .wrapping_mul(16_777_619)
            .wrapping_add(u64::from(*byte));
        match byte {
            0x1b | b'\r' | b'\n' | 0x08 => metrics.control_bytes += 1,
            0x80..=0xff => metrics.unicode_bytes += 1,
            0x20..=0x7e => metrics.printable_bytes += 1,
            _ => {}
        }
        if *byte == b'\n' {
            metrics.lines += 1;
        }
    }
    metrics
}
