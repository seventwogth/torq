use std::env;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use torq_runtime::{TorManager, TorRuntimeConfig};

#[tokio::main]
async fn main() -> Result<()> {
    let options = CliOptions::parse()?;

    let mut config =
        TorRuntimeConfig::new(&options.tor_path, &options.log_path).with_args(options.tor_args);
    config.working_dir = options.working_dir;
    config.append_log_argument = options.append_log_argument;

    let manager = TorManager::new(config).await?;
    let mut events = manager.subscribe_events();

    tokio::spawn(async move {
        while let Some(message) = next_event_line(&mut events).await {
            println!("{message}");
        }
    });

    manager.start().await?;

    println!("torq runtime CLI");
    println!("tor path: {}", options.tor_path.display());
    println!("log path: {}", options.log_path.display());
    println!("commands: start | stop | restart | newnym | state | exit");

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        match line.trim().to_ascii_lowercase().as_str() {
            "" => {}
            "start" => manager.start().await?,
            "stop" => manager.stop().await?,
            "restart" => manager.restart().await?,
            "newnym" => manager.new_identity().await?,
            "state" => println!("[state] {:?}", manager.current_state()),
            "exit" | "quit" => break,
            other => {
                println!("unknown command: {other}");
            }
        }
    }

    if manager.current_state().is_running {
        let _ = manager.stop().await;
    }

    Ok(())
}

struct CliOptions {
    tor_path: PathBuf,
    log_path: PathBuf,
    tor_args: Vec<String>,
    working_dir: Option<PathBuf>,
    append_log_argument: bool,
}

impl CliOptions {
    fn parse() -> Result<Self> {
        let mut args = env::args().skip(1);

        let mut tor_path = env::var_os("TORQ_TOR_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tor.exe"));
        let mut tor_path_set = false;
        let mut log_path = PathBuf::from("tor.log");
        let mut log_path_set = false;
        let mut tor_args = Vec::new();
        let mut working_dir = None;
        let mut append_log_argument = true;

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                "--working-dir" => {
                    let value = args.next().context("--working-dir expects a path")?;
                    working_dir = Some(PathBuf::from(value));
                }
                "--no-managed-log" => {
                    append_log_argument = false;
                }
                "--" => {
                    tor_args.extend(args);
                    break;
                }
                _ if !tor_path_set => {
                    tor_path = PathBuf::from(argument);
                    tor_path_set = true;
                }
                _ if !log_path_set => {
                    log_path = PathBuf::from(argument);
                    log_path_set = true;
                }
                _ => {
                    bail!("unexpected argument: {argument}. Use `--` before extra tor args.");
                }
            }
        }

        Ok(Self {
            tor_path,
            log_path,
            tor_args,
            working_dir,
            append_log_argument,
        })
    }
}

async fn next_event_line(events: &mut broadcast::Receiver<torq_core::TorEvent>) -> Option<String> {
    loop {
        match events.recv().await {
            Ok(event) => return Some(format!("[event] {event:?}")),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                return Some(format!("[event] lagged and skipped {skipped} event(s)"));
            }
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  cargo run -p torq-runtime -- [tor-path] [log-path] [--working-dir DIR] [--no-managed-log] [-- extra tor args]");
    println!();
    println!("Examples:");
    println!("  cargo run -p torq-runtime -- C:\\Tor\\tor.exe .\\tor.log");
    println!("  cargo run -p torq-runtime -- cmd.exe .\\tor.log -- /C scripts\\mock-tor.cmd");
    println!();
    println!("If omitted, tor-path defaults to TORQ_TOR_EXE or tor.exe.");
}
