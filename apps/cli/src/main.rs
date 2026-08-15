#![forbid(unsafe_code)]

use std::fs;

use cli::{CliConfig, ExitCode};

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
