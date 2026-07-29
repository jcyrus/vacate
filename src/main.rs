//! portkill — find and kill whatever is squatting on a port.

mod cli;
mod fuzzy;
mod kill;
mod ports;
mod process;
mod tui;
mod ui;

use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "portkill",
    version,
    about = "Find and kill whatever is squatting on a port.",
    after_help = "Run with no PORT to browse every listening port interactively."
)]
struct Args {
    /// Port to inspect and free. Omit to open the interactive browser.
    #[arg(value_name = "PORT")]
    port: Option<u16>,

    /// Send SIGKILL instead of SIGTERM, without confirming.
    #[arg(short, long)]
    force: bool,

    /// Skip the confirmation prompt, but still send SIGTERM.
    #[arg(short = 'y', long = "yes")]
    assume_yes: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let result = match args.port {
        Some(port) => cli::run(port, args.force, args.assume_yes),
        None => tui::run().map(|()| 0),
    };

    match result {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("portkill: {err:#}");
            ExitCode::FAILURE
        }
    }
}
