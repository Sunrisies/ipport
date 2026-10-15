use std::{
    io::{self, Write},
    net::SocketAddr,
};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use ipport::scanner::{self, Filter, Protocol, Snapshot};

#[derive(Parser)]
#[command(
    version,
    about = "Inspect local TCP/UDP ports and their owning processes"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List listening TCP ports and bound UDP ports (the default command).
    List(ListArgs),
    /// Show details for a local port, including active TCP connections.
    Show {
        port: u16,
        #[arg(long)]
        json: bool,
    },
    /// Start the local web dashboard.
    Web {
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: SocketAddr,
    },
}

#[derive(Args, Default)]
struct ListArgs {
    #[arg(long)]
    port: Option<u16>,
    #[arg(long, value_enum)]
    protocol: Option<Protocol>,
    #[arg(long)]
    pid: Option<u32>,
    /// Search addresses, process names, executable paths, or users.
    #[arg(long)]
    search: Option<String>,
    /// Include non-listening TCP connections.
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    match Cli::parse()
        .command
        .unwrap_or(Command::List(ListArgs::default()))
    {
        Command::Web { bind } => ipport::web::serve(bind),
        Command::List(args) => {
            let snapshot = scanner::scan(&Filter {
                port: args.port,
                protocol: args.protocol,
                pid: args.pid,
                search: args.search,
                all: args.all,
            })?;
            print_snapshot(&snapshot, args.json, false)
        }
        Command::Show { port, json } => {
            let snapshot = scanner::scan(&Filter {
                port: Some(port),
                all: true,
                ..Filter::default()
            })?;
            print_snapshot(&snapshot, json, true)
        }
    }
}

fn print_snapshot(snapshot: &Snapshot, json: bool, details: bool) -> Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    if json {
        serde_json::to_writer_pretty(&mut out, snapshot)?;
        writeln!(out)?;
    } else {
        writeln!(
            out,
            "{:<6} {:<5} {:<7} {:<16} {:<28} {:<9} PROCESS",
            "PORT", "PROTO", "FAMILY", "STATE", "LOCAL ADDRESS", "PID"
        )?;
        for port in &snapshot.ports {
            let pids = port
                .processes
                .iter()
                .map(|p| p.pid.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let names = port
                .processes
                .iter()
                .map(|p| p.name.as_deref().unwrap_or("unknown"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                out,
                "{:<6} {:<5} {:<7} {:<16} {:<28} {:<9} {}",
                port.local_port,
                port.protocol.to_string(),
                port.family,
                port.state,
                port.local_endpoint,
                if pids.is_empty() { "-" } else { &pids },
                if names.is_empty() { "unknown" } else { &names }
            )?;
            if details {
                writeln!(
                    out,
                    "       Bind scope: {}  Remote: {}",
                    port.exposure,
                    port.remote_endpoint.as_deref().unwrap_or("-")
                )?;
                for process in &port.processes {
                    writeln!(
                        out,
                        "       PID {}  User: {}\n       Executable: {}",
                        process.pid,
                        process.user.as_deref().unwrap_or("unavailable"),
                        process.executable.as_deref().unwrap_or("unavailable")
                    )?;
                }
            }
        }
        if snapshot.ports.is_empty() {
            writeln!(out, "No matching ports visible to the current user.")?;
        }
        writeln!(
            out,
            "\n{} sockets · {} · {} ms",
            snapshot.ports.len(),
            snapshot.platform,
            snapshot.elapsed_ms
        )?;
    }
    out.flush()?;
    for warning in &snapshot.warnings {
        eprintln!("Warning: {warning}");
    }
    Ok(())
}
