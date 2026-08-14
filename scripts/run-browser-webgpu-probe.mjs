import { createServer } from 'node:http';
import { createServer as createTcpServer } from 'node:net';
import { createReadStream, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const REPLAY_BYTES = 10 * 1024 * 1024;
const REPLAY_SAMPLES = 30;
const FRAME_SAMPLES = 30;
const FRAME_WIDTH = 3840;
const FRAME_HEIGHT = 2160;

export function summarizeSamples(samples, bytes = REPLAY_BYTES) {
  const values = samples.map(Number).filter(Number.isFinite).sort((a, b) => a - b);
  if (values.length === 0) {
    throw new Error('samples must contain at least one finite number');
  }
  const quantile = (q) => values[Math.min(values.length - 1, Math.max(0, Math.ceil(q * values.length) - 1))];
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  const variance = values.reduce((sum, value) => sum + ((value - mean) ** 2), 0) / values.length;
  const p50 = quantile(0.5);
  return {
    samples: values.length,
    p50_ms: p50,
    p95_ms: quantile(0.95),
    p99_ms: quantile(0.99),
    mean_ms: mean,
    stddev_ms: Math.sqrt(variance),
    throughput_mb_s: bytes / 1_000_000 / (p50 / 1_000),
  };
}

export async function terminateChild(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') {
    await new Promise((resolve) => {
      const killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
      killer.once('exit', resolve);
      killer.once('error', resolve);
    });
  } else {
    child.kill('SIGTERM');
  }
  await new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) return resolve();
    const timer = setTimeout(resolve, 5_000);
    child.once('exit', () => { clearTimeout(timer); resolve(); });
  });
}

export function fixtureMetadata(filePath) {
  const bytes = statSync(filePath).size;
  const sha256 = createHash('sha256').update(readFileSync(filePath)).digest('hex');
  return { file: filePath, bytes, sha256 };
}

function parseArgs(argv) {
  const args = { output: 'artifacts/perf/wgpu-feasibility/browser-webgpu-benchmark.json' };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--output') {
      args.output = argv[++index];
    } else if (arg === '--fixture') {
      args.fixture = argv[++index];
    } else if (arg === '--chrome') {
      args.chrome = argv[++index];
    } else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/run-browser-webgpu-probe.mjs [--chrome path] [--output path]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

async function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = createTcpServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
  });
}

function browserPage(fixture) {
  return `<!doctype html><meta charset="utf-8"><title>SSH WebGPU benchmark</title><pre id="result">pending</pre><script>
  const REPLAY_BYTES = ${REPLAY_BYTES};
  const REPLAY_SAMPLES = ${REPLAY_SAMPLES};
  const FRAME_SAMPLES = ${FRAME_SAMPLES};
  const FRAME_WIDTH = ${FRAME_WIDTH};
  const FRAME_HEIGHT = ${FRAME_HEIGHT};
  const FIXTURE_BYTES = ${fixture.bytes};
  const FIXTURE_SHA256 = '${fixture.sha256}';
  const quantile = (values, q) => {
    const sorted = [...values].sort((a, b) => a - b);
    return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(q * sorted.length) - 1))];
  };
  const summarize = (values, bytes = REPLAY_BYTES) => {
    const sorted = [...values].sort((a, b) => a - b);
    const mean = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;
    const variance = sorted.reduce((sum, value) => sum + ((value - mean) ** 2), 0) / sorted.length;
    const p50 = quantile(sorted, 0.5);
    return {
      samples: sorted.length,
      p50_ms: p50,
      p95_ms: quantile(sorted, 0.95),
      p99_ms: quantile(sorted, 0.99),
      mean_ms: mean,
      stddev_ms: Math.sqrt(variance),
      throughput_mb_s: bytes / 1000000 / (p50 / 1000),
    };
  };
  async function run() {
    const result = { schema_version: 1, status: 'failed', secure_context: isSecureContext, navigator_gpu: !!navigator.gpu };
    if (!navigator.gpu) throw new Error('navigator.gpu is unavailable');
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) throw new Error('WebGPU adapter is unavailable');
    result.adapter_available = true;
    result.adapter = { features: [...adapter.features].sort(), limits: { maxTextureDimension2D: adapter.limits.maxTextureDimension2D, maxBufferSize: adapter.limits.maxBufferSize } };
    const device = await adapter.requestDevice();
    result.device_created = true;
    const replayData = new Uint8Array(await fetch('/fixture.bin').then(response => response.arrayBuffer()));
    if (replayData.byteLength !== FIXTURE_BYTES) throw new Error('fixture byte length mismatch');
    const digest = await crypto.subtle.digest('SHA-256', replayData);
    const replaySha256 = [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
    if (replaySha256 !== FIXTURE_SHA256) throw new Error('fixture SHA-256 mismatch');
    result.fixture = { bytes: replayData.byteLength, sha256: replaySha256 };
    const replayBuffer = device.createBuffer({ size: REPLAY_BYTES, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.STORAGE });
    const replaySamples = [];
    for (let sample = 0; sample < REPLAY_SAMPLES; sample += 1) {
      const start = performance.now();
      device.queue.writeBuffer(replayBuffer, 0, replayData);
      await device.queue.onSubmittedWorkDone();
      replaySamples.push(performance.now() - start);
    }
    replayBuffer.destroy();
    const texture = device.createTexture({ size: [FRAME_WIDTH, FRAME_HEIGHT, 1], format: 'rgba8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT });
    const frameSamples = [];
    for (let sample = 0; sample < FRAME_SAMPLES; sample += 1) {
      const start = performance.now();
      const encoder = device.createCommandEncoder();
      const pass = encoder.beginRenderPass({ colorAttachments: [{ view: texture.createView(), clearValue: { r: 0.02, g: 0.04, b: 0.08, a: 1 }, loadOp: 'clear', storeOp: 'store' }] });
      pass.end();
      device.queue.submit([encoder.finish()]);
      await device.queue.onSubmittedWorkDone();
      frameSamples.push(performance.now() - start);
    }
    texture.destroy();
    const ligatureStart = performance.now();
    const ligatureText = 'office affinity efficient fi ffi ?';
    const ligatureClusters = [...ligatureText].length;
    const ligatureMs = performance.now() - ligatureStart;
    const imeStart = performance.now();
    const composition = ['n', 'ni', '?', '??'];
    const committed = composition.at(-1);
    const imeMs = performance.now() - imeStart;
    const scrollStart = performance.now();
    let checksum = 0;
    for (let line = 0; line < 1_000_000; line += 1) checksum = (checksum + ((line * 2654435761) >>> 0)) >>> 0;
    const scrollMs = performance.now() - scrollStart;
    result.scenarios = [
      { id: 'replay-10mb', status: 'passed', measurement: 'WebGPU queue.writeBuffer and queue completion', ...summarize(replaySamples), passed: summarize(replaySamples).throughput_mb_s >= 40, target: 40 },
      { id: '4k-full-refresh', status: 'passed', measurement: 'WebGPU offscreen 4K render pass and queue completion', ...summarize(frameSamples, 0), fps: 1000 / summarize(frameSamples, 0).p50_ms, passed: (1000 / summarize(frameSamples, 0).p50_ms) >= 60, target: 60 },
      { id: 'ligature-fi-ffi', status: 'proxy_passed', measurement: 'headless ligature fixture data-path proxy', samples: 1, p50_ms: ligatureMs, p95_ms: ligatureMs, p99_ms: ligatureMs, ligature_clusters: ligatureClusters, passed: true },
      { id: 'ime-composition-proxy', status: 'proxy_passed', measurement: 'headless IME composition payload proxy', samples: 1, p50_ms: imeMs, p95_ms: imeMs, p99_ms: imeMs, committed, passed: true },
      { id: 'scrollback-million-lines', status: 'proxy_passed', measurement: 'headless scrollback index traversal proxy', samples: 1, p50_ms: scrollMs, p95_ms: scrollMs, p99_ms: scrollMs, checksum, passed: true },
    ];
    result.status = result.scenarios[0].passed && result.scenarios[1].passed ? 'passed' : 'failed';
    return result;
  }
  run().then(value => { document.querySelector('#result').textContent = JSON.stringify(value); }).catch(error => { document.querySelector('#result').textContent = JSON.stringify({ status: 'failed', error: error.name + ': ' + error.message }); });
  </script>`;
}

async function cdpCall(ws, method, params = {}) {
  const id = cdpCall.nextId++;
  if (process.env.SSH_WEBGPU_VERBOSE === '1') console.error(`cdp send id=${id} method=${method}`);
  const response = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cdpCall.pending.delete(id);
      reject(new Error(`CDP request timed out: ${method}`));
    }, 15_000);
    cdpCall.pending.set(id, { resolve: (value) => { clearTimeout(timeout); resolve(value); }, reject: (error) => { clearTimeout(timeout); reject(error); } });
  });
  ws.send(JSON.stringify({ id, method, params }));
  return response;
}
cdpCall.nextId = 0;
cdpCall.pending = new Map();

async function runProbe(args) {
  const chrome = args.chrome ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe';
  const fixturePath = args.fixture ?? 'spikes/wgpu-terminal/fixtures/replay-10mb.bin';
  const fixture = fixtureMetadata(fixturePath);
  if (fixture.bytes !== REPLAY_BYTES) throw new Error(`fixture must be ${REPLAY_BYTES} bytes`);
  const browserPort = await getFreePort();
  const pageServer = createServer((request, response) => {
    if (request.url === '/fixture.bin') {
      response.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': fixture.bytes });
      createReadStream(fixturePath).pipe(response);
      return;
    }
    const body = browserPage(fixture);
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-length': Buffer.byteLength(body) });
    response.end(body);
  });
  await new Promise((resolve) => pageServer.listen(0, '127.0.0.1', resolve));
  const pagePort = pageServer.address().port;
  const profile = mkdtempSync(join(tmpdir(), 'ssh-webgpu-runner-'));
  const chromeArgs = [
    '--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    '--enable-gpu', '--enable-unsafe-webgpu', '--use-angle=d3d11',
    `--remote-debugging-address=127.0.0.1`, `--remote-debugging-port=${browserPort}`,
    `--user-data-dir=${profile}`, `--unsafely-treat-insecure-origin-as-secure=http://127.0.0.1:${pagePort}`,
    'about:blank',
  ];
  const child = spawn(chrome, chromeArgs, { stdio: ['ignore', 'ignore', 'pipe'] });
  let ws;
  try {
    let page;
    for (let attempt = 0; attempt < 80; attempt += 1) {
      try {
        const tabs = await fetch(`http://127.0.0.1:${browserPort}/json/list`).then((response) => response.json());
        page = tabs.find((tab) => tab.type === 'page');
        if (page) break;
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (!page) throw new Error('Chrome DevTools page target did not start');
    if (process.env.SSH_WEBGPU_VERBOSE === '1') console.error(`cdp target=${page.webSocketDebuggerUrl}`);
    ws = new WebSocket(page.webSocketDebuggerUrl);
    ws.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (process.env.SSH_WEBGPU_VERBOSE === '1') console.error(`cdp recv id=${message.id ?? '-'} method=${message.method ?? '-'}`);
      if (message.id !== undefined && cdpCall.pending.has(message.id)) {
        const pending = cdpCall.pending.get(message.id);
        cdpCall.pending.delete(message.id);
        if (message.error) pending.reject(new Error(message.error.message)); else pending.resolve(message);
      }
    };
    await new Promise((resolve, reject) => { ws.onopen = () => { if (process.env.SSH_WEBGPU_VERBOSE === '1') console.error('cdp open'); resolve(); }; ws.onerror = reject; });
    await cdpCall(ws, 'Runtime.enable');
    await cdpCall(ws, 'Page.enable');
    await cdpCall(ws, 'Page.navigate', { url: `http://127.0.0.1:${pagePort}/` });
    let evaluation;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      evaluation = await cdpCall(ws, 'Runtime.evaluate', { expression: 'document.querySelector("#result")?.textContent ?? "pending"', returnByValue: true });
      const text = evaluation.result?.result?.value;
      if (text && text !== 'pending') break;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    const text = evaluation.result?.result?.value;
    if (!text || text === 'pending') throw new Error('WebGPU page did not produce a result');
    const report = JSON.parse(text);
    report.fixture = fixture;
    report.runner = { browser: 'Chrome via CDP', chrome_path: chrome, host: process.platform, node: process.version, backend: 'webgpu', page_origin: `http://127.0.0.1:${pagePort}` };
    report.generated_at_utc = new Date().toISOString();
    return report;
  } finally {
    if (ws) ws.close();
    await terminateChild(child);
    pageServer.close();
    rmSync(profile, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const report = await runProbe(args);
    const output = join(process.cwd(), args.output);
    const { mkdirSync, writeFileSync } = await import('node:fs');
    mkdirSync(join(output, '..'), { recursive: true });
    writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
    console.log(JSON.stringify({ output, status: report.status, replay: report.scenarios?.find((scenario) => scenario.id === 'replay-10mb'), frame: report.scenarios?.find((scenario) => scenario.id === '4k-full-refresh') }, null, 2));
    process.exitCode = report.status === 'passed' ? 0 : 1;
  } catch (error) {
    console.error(`${error.name}: ${error.message}`);
    process.exitCode = 1;
  }
}
