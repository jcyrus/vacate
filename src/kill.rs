//! Signal delivery, with the guardrails you want on a tool whose whole job
//! is killing things.

use std::fmt;
use std::io;

use anyhow::{Result, bail};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Ask politely. Lets the process run its shutdown handlers.
    Term,
    /// Don't ask.
    Kill,
}

impl Signal {
    fn number(self) -> libc::c_int {
        match self {
            Self::Term => libc::SIGTERM,
            Self::Kill => libc::SIGKILL,
        }
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Term => "SIGTERM",
            Self::Kill => "SIGKILL",
        })
    }
}

/// Send `signal` to `pid`.
pub fn send(pid: u32, signal: Signal) -> Result<()> {
    // kill(0) signals our whole process group and kill(1) goes after init;
    // neither is ever what someone freeing up port 3000 meant to do.
    if pid <= 1 {
        bail!("refusing to signal PID {pid}");
    }
    if pid == std::process::id() {
        bail!("refusing to signal vacate itself");
    }

    if unsafe { libc::kill(pid as libc::pid_t, signal.number()) } == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => bail!("process {pid} is already gone"),
        Some(libc::EPERM) => bail!("not permitted to signal process {pid} — try with sudo"),
        _ => bail!("failed to signal process {pid}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_init_and_process_group() {
        assert!(send(0, Signal::Term).is_err());
        assert!(send(1, Signal::Term).is_err());
    }

    #[test]
    fn refuses_suicide() {
        let err = send(std::process::id(), Signal::Kill).unwrap_err();
        assert!(err.to_string().contains("itself"));
    }

    #[test]
    fn reports_missing_process() {
        // Reap a real child so we know the PID is dead rather than recycled.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();

        let err = send(pid, Signal::Term).unwrap_err();
        assert!(err.to_string().contains("already gone"), "got: {err}");
    }

    #[test]
    fn signal_names_are_stable() {
        assert_eq!(Signal::Term.to_string(), "SIGTERM");
        assert_eq!(Signal::Kill.to_string(), "SIGKILL");
    }
}
