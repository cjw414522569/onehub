#![forbid(unsafe_code)]

use std::fs;

use cli::{parse_target, CliConfig, ExitCode, ForwardSpec, ProxyChainSpec, ProxyHop, SftpSpec};

/// The CLI version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn print_usage() {
    println!(
        "usage: ssh-cli [--version] [--help]\n       ssh-cli config --check <path>\n       ssh-cli --config <path> <alias> exec <command>"
    );
}

fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None => {
            print_usage();
            ExitCode::Ok.code()
        }
        Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::Ok.code()
        }
        Some("--version") | Some("-V") => {
            println!("ssh-cli {VERSION}");
            ExitCode::Ok.code()
        }
        Some("cap") => cap_command(&args[1..]),
        Some("config") => {
            if args.get(1).map(String::as_str) != Some("--check") {
                print_usage();
                return ExitCode::Usage.code();
            }
            match args.get(2) {
                Some(path) => config_check(path),
                None => {
                    print_usage();
                    ExitCode::Usage.code()
                }
            }
        }
        Some("--config") => {
            // Expect: --config <path> <alias> exec <command>
            let path = match args.get(1) {
                Some(path) => path,
                None => {
                    print_usage();
                    return ExitCode::Usage.code();
                }
            };
            let alias = match args.get(2) {
                Some(alias) => alias,
                None => {
                    print_usage();
                    return ExitCode::Usage.code();
                }
            };
            exec_alias(
                path,
                alias,
                args.get(3).map(String::as_str),
                args.get(4).map(String::as_str),
            )
        }
        Some(_) => {
            print_usage();
            ExitCode::Usage.code()
        }
    }
}

/// `cap` command: prints the CLI capability specs (forward / sftp / proxy)
/// built from the same shared core configs the GUI uses.
fn cap_command(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("forward") => {
            let listen = args
                .iter()
                .position(|a| a == "--listen")
                .and_then(|i| args.get(i + 1));
            let target = args
                .iter()
                .position(|a| a == "--target")
                .and_then(|i| args.get(i + 1));
            let (listen, target) = match (listen, target) {
                (Some(l), Some(t)) => (l, t),
                _ => {
                    print_usage();
                    return ExitCode::Usage.code();
                }
            };
            let parsed = listen
                .split_once(':')
                .and_then(|(ip, port)| {
                    Some((
                        ip.parse::<std::net::IpAddr>().ok()?,
                        port.parse::<u16>().ok()?,
                    ))
                })
                .and_then(|(ip, port)| {
                    let (target_host, target_port) = target.rsplit_once(':')?;
                    let target_port = target_port.parse::<u16>().ok()?;
                    Some((ip, port, target_host.to_owned(), target_port))
                });
            match parsed {
                Some((bind_ip, listen_port, target_host, target_port)) => {
                    let spec = ForwardSpec {
                        bind_ip,
                        listen_port,
                        target_host,
                        target_port,
                        max_connections: 0,
                    };
                    println!(
                        "forward spec: {} -> {}:{} scope={:?}",
                        spec.to_config().listen,
                        spec.target_host,
                        spec.target_port,
                        spec.bind_scope()
                    );
                    ExitCode::Ok.code()
                }
                None => {
                    eprintln!("usage: forward --listen <ip:port> --target <host:port>");
                    ExitCode::Usage.code()
                }
            }
        }
        Some("sftp") => {
            let spec = SftpSpec::default();
            println!(
                "sftp spec: chunk={} in_flight={}",
                spec.chunk_size, spec.max_in_flight
            );
            ExitCode::Ok.code()
        }
        Some("proxy") => {
            let chain = args
                .iter()
                .position(|a| a == "--chain")
                .and_then(|i| args.get(i + 1));
            let target = args
                .iter()
                .position(|a| a == "--target")
                .and_then(|i| args.get(i + 1));
            let (chain, target) = match (chain, target) {
                (Some(c), Some(t)) => (c, t),
                _ => {
                    print_usage();
                    return ExitCode::Usage.code();
                }
            };
            let hops: Vec<ProxyHop> = chain
                .split(',')
                .filter_map(|hop| {
                    let hop = hop.trim();
                    let (socks5, host_port) = if let Some(rest) = hop.strip_prefix("socks5://") {
                        (true, rest)
                    } else {
                        (false, hop.strip_prefix("http://")?)
                    };
                    let (host, port) = host_port.rsplit_once(':')?;
                    Some(ProxyHop {
                        host: host.to_owned(),
                        port: port.parse().ok()?,
                        socks5,
                        username: None,
                    })
                })
                .collect();
            let spec = ProxyChainSpec { hops };
            let target = parse_target(target);
            let target_port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(22);
            match spec.first_hop_wire(&target, target_port) {
                Ok(wire) => {
                    let hex: Vec<String> = wire.iter().map(|b| format!("{b:02x}")).collect();
                    println!("proxy chain first-hop: {}", hex.join(" "));
                    ExitCode::Ok.code()
                }
                Err(error) => {
                    eprintln!("proxy error: {error}");
                    ExitCode::Config.code()
                }
            }
        }
        _ => {
            print_usage();
            ExitCode::Usage.code()
        }
    }
}

fn config_check(path: &str) -> i32 {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("config error: cannot read {path}: {error}");
            return ExitCode::Config.code();
        }
    };
    match CliConfig::parse(&text) {
        Ok(config) => {
            println!("config valid: {} host(s)", config.hosts.len());
            ExitCode::Ok.code()
        }
        Err(error) => {
            eprintln!("config error: {error:?} in {path}");
            ExitCode::Config.code()
        }
    }
}

fn exec_alias(path: &str, alias: &str, exec: Option<&str>, command: Option<&str>) -> i32 {
    if exec != Some("exec") || command.is_none() {
        print_usage();
        return ExitCode::Usage.code();
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("config error: cannot read {path}: {error}");
            return ExitCode::Config.code();
        }
    };
    let config = match CliConfig::parse(&text) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("config error: {error:?} in {path}");
            return ExitCode::Config.code();
        }
    };
    if config.get(alias).is_none() {
        eprintln!("config error: unknown host alias '{alias}'");
        return ExitCode::Config.code();
    }
    // The connect backend is wired in a later control row; until then the
    // CLI reports a stable connection error.
    eprintln!("connection error: SSH backend not available in this build");
    ExitCode::Connect.code()
}
