//! Direct mode: `portkill 8080`. Inspect, confirm, terminate.

use std::io::{self, IsTerminal, Write};

use anyhow::{Result, bail};

use crate::kill::{self, Signal};
use crate::ports::{self, PortEntry};
use crate::process::human_bytes;

/// Exit code used when the port turned out to be free.
pub const EXIT_NOT_FOUND: i32 = 1;
/// Exit code used when the user declined at the confirmation prompt.
pub const EXIT_CANCELLED: i32 = 2;
/// Exit code used when a process was found but could not be signalled.
pub const EXIT_KILL_FAILED: i32 = 3;

pub fn run(port: u16, force: bool, assume_yes: bool) -> Result<i32> {
    let entries = ports::scan_port(port)?;
    if entries.is_empty() {
        eprintln!("Nothing is listening on port {port}.");
        return Ok(EXIT_NOT_FOUND);
    }

    print_table(&entries);

    let signal = if force { Signal::Kill } else { Signal::Term };
    let needs_confirmation = !force && !assume_yes;
    if needs_confirmation && !confirm(&entries, signal)? {
        println!("Cancelled.");
        return Ok(EXIT_CANCELLED);
    }

    // Report per process rather than bailing on the first failure: killing
    // three of four workers is still useful, and the user needs to know which
    // one survived.
    let mut failed = false;
    for entry in &entries {
        match kill::send(entry.pid, signal) {
            Ok(()) => println!("Sent {signal} to {} (PID {}).", entry.name, entry.pid),
            Err(err) => {
                failed = true;
                eprintln!("{err}");
            }
        }
    }

    // Anything short of "every process signalled" is a failure to a script:
    // the port may well still be occupied.
    Ok(if failed { EXIT_KILL_FAILED } else { 0 })
}

fn print_table(entries: &[PortEntry]) {
    // Size the variable-width columns to their contents so nothing wraps.
    let name_width = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let user_width = entries
        .iter()
        .map(|e| e.user.len())
        .max()
        .unwrap_or(4)
        .max(4);

    println!(
        "{:<6} {:<5} {:<7} {:<name_width$} {:<user_width$} MEMORY",
        "PORT", "PROTO", "PID", "PROCESS", "USER"
    );
    for entry in entries {
        println!(
            "{:<6} {:<5} {:<7} {:<name_width$} {:<user_width$} {}",
            entry.port,
            entry.proto.to_string(),
            entry.pid,
            entry.name,
            entry.user,
            human_bytes(entry.memory),
        );
    }
}

/// Ask before pulling the trigger. Returns whether to proceed.
fn confirm(entries: &[PortEntry], signal: Signal) -> Result<bool> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        // Nobody is there to answer, so hanging on a read would be worse than
        // failing loudly.
        bail!("stdin is not a terminal — pass --yes to confirm or --force to skip the prompt");
    }

    let target = match entries {
        [only] => format!("{} (PID {})", only.name, only.pid),
        _ => format!("{} processes", entries.len()),
    };
    print!("\nSend {signal} to {target}? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    stdin.read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}
