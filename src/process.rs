//! Per-PID details (name, owner, resident memory) straight from the OS.
//!
//! Deliberately not `sysinfo`: we only ever look up the handful of PIDs that
//! actually hold a socket, so a whole-system process scan would be pure waste.

use std::ffi::CStr;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
compile_error!(
    "portkill supports macOS and Linux only — process lookup has no implementation for this target"
);

/// What we can learn about a process that holds a socket.
pub struct ProcInfo {
    pub name: String,
    pub uid: u32,
    /// Resident set size, in bytes.
    pub rss: u64,
}

/// Resolve the login name for a uid, falling back to the numeric id.
pub fn username(uid: u32) -> String {
    // Single-threaded process, so the non-`_r` variant is fine; we copy the
    // string out before anything else can touch the static buffer.
    let pw = unsafe { libc::getpwuid(uid as libc::uid_t) };
    if pw.is_null() {
        return uid.to_string();
    }
    let name = unsafe { (*pw).pw_name };
    if name.is_null() {
        return uid.to_string();
    }
    unsafe { CStr::from_ptr(name) }
        .to_str()
        .map(str::to_owned)
        .unwrap_or_else(|_| uid.to_string())
}

#[cfg(target_os = "macos")]
pub fn info(pid: u32) -> Option<ProcInfo> {
    use std::mem;

    let mut ti: libc::proc_taskallinfo = unsafe { mem::zeroed() };
    let size = mem::size_of::<libc::proc_taskallinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKALLINFO,
            0,
            (&raw mut ti).cast(),
            size,
        )
    };
    // A short read means the struct wasn't filled (dead process, or we lack
    // the privileges to look at it).
    if n < size {
        return None;
    }

    // `pbi_name` is the (longer) accounting name and is empty for some
    // processes; `pbi_comm` is the truncated-but-always-present fallback.
    let name = cstr_field(&ti.pbsd.pbi_name).or_else(|| cstr_field(&ti.pbsd.pbi_comm))?;

    Some(ProcInfo {
        name,
        uid: ti.pbsd.pbi_uid,
        rss: ti.ptinfo.pti_resident_size,
    })
}

/// Read a fixed-size, NUL-padded C char array into a `String`.
#[cfg(target_os = "macos")]
fn cstr_field(buf: &[libc::c_char]) -> Option<String> {
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), buf.len()) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = String::from_utf8_lossy(&bytes[..end]).into_owned();
    (!s.is_empty()).then_some(s)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn info(pid: u32) -> Option<ProcInfo> {
    // One read of /proc/<pid>/status covers all three fields.
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;

    let mut name = None;
    let mut uid = None;
    let mut rss = 0; // kernel threads have no VmRSS line at all

    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Name" => name = Some(value.to_owned()),
            // "Uid: <real> <effective> <saved> <fs>" — the real uid is the owner.
            "Uid" => uid = value.split_whitespace().next().and_then(|v| v.parse().ok()),
            // "VmRSS: <n> kB"
            "VmRSS" => {
                rss = value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024;
            }
            _ => {}
        }
        if name.is_some() && uid.is_some() && rss != 0 {
            break;
        }
    }

    Some(ProcInfo {
        name: name?,
        uid: uid?,
        rss,
    })
}

/// Format a byte count for a fixed-width table column.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_is_resolvable() {
        let me = std::process::id();
        let info = info(me).expect("we can always inspect ourselves");
        assert!(!info.name.is_empty());
        assert!(info.rss > 0, "a running process has resident memory");
    }

    #[test]
    fn dead_pid_resolves_to_nothing() {
        // PID 0 is never a normal user process on Linux or macOS.
        assert!(info(0).is_none());
    }

    #[test]
    fn root_has_a_name() {
        assert_eq!(username(0), "root");
    }

    #[test]
    fn bytes_scale_to_readable_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 K");
        assert_eq!(human_bytes(20 * 1024), "20 K");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 M");
        assert_eq!(human_bytes(1536 * 1024 * 1024), "1.5 G");
    }
}
