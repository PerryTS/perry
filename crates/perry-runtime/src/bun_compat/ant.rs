//! Claude Code's vendor `Bun.ant` hooks.
//!
//! These APIs are not part of Bun's public compatibility surface.  They are
//! nevertheless useful as small, conservative OS primitives: peer credential
//! lookup never takes ownership of the descriptor and every unsupported or
//! failed lookup returns `null`.

use crate::value::JSValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MemoryPressureLevel {
    Normal,
    Warning,
    Critical,
}

impl MemoryPressureLevel {
    fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Normal => b"normal",
            Self::Warning => b"warning",
            Self::Critical => b"critical",
        }
    }
}

/// Keep acquisition separate from JS conversion so platform sources can be
/// tested without manufacturing process-wide memory pressure.
trait MemoryPressureSource {
    fn current_level(&self) -> Option<MemoryPressureLevel>;
}

struct PlatformMemoryPressureSource;

impl MemoryPressureSource for PlatformMemoryPressureSource {
    fn current_level(&self) -> Option<MemoryPressureLevel> {
        platform_memory_pressure_level()
    }
}

fn null() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn fd_from_value(value: f64) -> Option<libc::c_int> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_int32() {
        return (value.as_int32() >= 0).then_some(value.as_int32());
    }
    if !value.is_number() {
        return None;
    }
    let number = value.as_number();
    if !number.is_finite()
        || number < 0.0
        || number.fract() != 0.0
        || number > libc::c_int::MAX as f64
    {
        return None;
    }
    Some(number as libc::c_int)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn linux_peer_credentials(fd: libc::c_int) -> Option<libc::ucred> {
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast::<libc::c_void>(),
            &mut len,
        )
    };
    if result != 0 || (len as usize) < std::mem::size_of::<libc::ucred>() {
        return None;
    }
    Some(unsafe { credentials.assume_init() })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(fd: libc::c_int) -> Option<u32> {
    linux_peer_credentials(fd).map(|credentials| credentials.uid)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_pid(fd: libc::c_int) -> Option<u32> {
    let pid = linux_peer_credentials(fd)?.pid;
    (pid > 0).then_some(pid as u32)
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn peer_uid(fd: libc::c_int) -> Option<u32> {
    let mut uid = 0 as libc::uid_t;
    let mut gid = 0 as libc::gid_t;
    (unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } == 0).then_some(uid as u32)
}

#[cfg(target_os = "macos")]
fn peer_pid(fd: libc::c_int) -> Option<u32> {
    let mut pid = 0 as libc::pid_t;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast::<libc::c_void>(),
            &mut len,
        )
    };
    (result == 0 && (len as usize) >= std::mem::size_of::<libc::pid_t>() && pid > 0)
        .then_some(pid as u32)
}

#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn peer_pid(_fd: libc::c_int) -> Option<u32> {
    None
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn peer_uid(_fd: libc::c_int) -> Option<u32> {
    None
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn peer_pid(_fd: libc::c_int) -> Option<u32> {
    None
}

/// `Bun.ant.getPeerUid(fd)` — effective UID for the peer of a local socket.
#[no_mangle]
pub extern "C" fn js_bun_ant_get_peer_uid(fd: f64) -> f64 {
    fd_from_value(fd)
        .and_then(peer_uid)
        .map(|uid| uid as f64)
        .unwrap_or_else(null)
}

/// `Bun.ant.getPeerPid(fd)` — PID for the peer when the OS exposes one.
#[no_mangle]
pub extern "C" fn js_bun_ant_get_peer_pid(fd: f64) -> f64 {
    fd_from_value(fd)
        .and_then(peer_pid)
        .map(|pid| pid as f64)
        .unwrap_or_else(null)
}

// Its production callers are the Linux/Android cgroup and sysinfo paths below;
// on other targets only the unit test uses it.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "android", test)),
    allow(dead_code)
)]
fn classify_availability(available: u64, limit: u64) -> Option<MemoryPressureLevel> {
    if limit == 0 {
        return None;
    }
    let available_percent = (available.min(limit) as u128 * 100) / limit as u128;
    Some(if available_percent <= 5 {
        MemoryPressureLevel::Critical
    } else if available_percent <= 15 {
        MemoryPressureLevel::Warning
    } else {
        MemoryPressureLevel::Normal
    })
}

#[cfg(any(test, target_os = "macos"))]
fn classify_dispatch_pressure_flag(flag: u32) -> Option<MemoryPressureLevel> {
    match flag {
        0x1 => Some(MemoryPressureLevel::Normal),
        0x2 => Some(MemoryPressureLevel::Warning),
        0x4 => Some(MemoryPressureLevel::Critical),
        _ => None,
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_meminfo(contents: &str) -> Option<(u64, u64)> {
    fn kib_value(contents: &str, key: &str) -> Option<u64> {
        let line = contents.lines().find(|line| line.starts_with(key))?;
        line.split_ascii_whitespace().nth(1)?.parse::<u64>().ok()
    }
    let total = kib_value(contents, "MemTotal:")?.checked_mul(1024)?;
    let available = kib_value(contents, "MemAvailable:")?.checked_mul(1024)?;
    Some((available, total))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn cgroup_v2_directory() -> Option<std::path::PathBuf> {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let relative = cgroup.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next()?;
        let controllers = fields.next()?;
        let path = fields.next()?;
        (hierarchy == "0" && controllers.is_empty()).then_some(path)
    })?;
    Some(std::path::Path::new("/sys/fs/cgroup").join(relative.trim_start_matches('/')))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_u64(path: &std::path::Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn cgroup_memory_level() -> Option<MemoryPressureLevel> {
    let directory = cgroup_v2_directory()?;
    let current = read_u64(&directory.join("memory.current"))?;
    let max = read_u64(&directory.join("memory.max"));
    let high = read_u64(&directory.join("memory.high"));
    let limit = match (max, high) {
        (Some(max), Some(high)) => max.min(high),
        (Some(limit), None) | (None, Some(limit)) => limit,
        (None, None) => return None,
    };
    classify_availability(limit.saturating_sub(current), limit)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn platform_memory_pressure_level() -> Option<MemoryPressureLevel> {
    let host = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|contents| parse_meminfo(&contents))
        .and_then(|(available, total)| classify_availability(available, total));
    [host, cgroup_memory_level()].into_iter().flatten().max()
}

#[cfg(target_os = "macos")]
fn platform_memory_pressure_level() -> Option<MemoryPressureLevel> {
    // This masked sysctl returns the public dispatch-memory-pressure flags,
    // not XNU's separate 0–100 memorystatus percentage.
    const NAME: &[u8] = b"kern.memorystatus_vm_pressure_level\0";
    let mut level = 0 as libc::c_uint;
    let mut len = std::mem::size_of::<libc::c_uint>();
    let result = unsafe {
        libc::sysctlbyname(
            NAME.as_ptr().cast::<libc::c_char>(),
            (&mut level as *mut libc::c_uint).cast::<libc::c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || len < std::mem::size_of::<libc::c_uint>() {
        return None;
    }
    classify_dispatch_pressure_flag(level)
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn platform_memory_pressure_level() -> Option<MemoryPressureLevel> {
    None
}

fn memory_pressure_level_from(source: &dyn MemoryPressureSource) -> Option<MemoryPressureLevel> {
    source.current_level()
}

/// `Bun.ant.memoryPressureLevel()` — current coarse OS pressure, or `null`
/// when the platform has no reliable source.
#[no_mangle]
pub extern "C" fn js_bun_ant_memory_pressure_level() -> f64 {
    match memory_pressure_level_from(&PlatformMemoryPressureSource) {
        Some(level) => super::boxed_str(level.as_bytes()),
        None => null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedSource(Option<MemoryPressureLevel>);

    impl MemoryPressureSource for FixedSource {
        fn current_level(&self) -> Option<MemoryPressureLevel> {
            self.0
        }
    }

    #[test]
    fn memory_pressure_source_covers_every_public_state_and_unavailable() {
        for expected in [
            Some(MemoryPressureLevel::Normal),
            Some(MemoryPressureLevel::Warning),
            Some(MemoryPressureLevel::Critical),
            None,
        ] {
            assert_eq!(memory_pressure_level_from(&FixedSource(expected)), expected);
        }
    }

    #[test]
    fn availability_thresholds_are_stable() {
        assert_eq!(
            classify_availability(16, 100),
            Some(MemoryPressureLevel::Normal)
        );
        assert_eq!(
            classify_availability(15, 100),
            Some(MemoryPressureLevel::Warning)
        );
        assert_eq!(
            classify_availability(5, 100),
            Some(MemoryPressureLevel::Critical)
        );
        assert_eq!(classify_availability(0, 0), None);
    }

    #[test]
    fn dispatch_pressure_flags_map_to_public_states() {
        assert_eq!(
            classify_dispatch_pressure_flag(0x1),
            Some(MemoryPressureLevel::Normal)
        );
        assert_eq!(
            classify_dispatch_pressure_flag(0x2),
            Some(MemoryPressureLevel::Warning)
        );
        assert_eq!(
            classify_dispatch_pressure_flag(0x4),
            Some(MemoryPressureLevel::Critical)
        );
        assert_eq!(classify_dispatch_pressure_flag(0), None);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    #[test]
    fn peer_credentials_accept_a_local_socket_and_reject_bad_descriptors() {
        use std::os::fd::AsRawFd;

        let (left, right) = std::os::unix::net::UnixStream::pair().expect("UnixStream pair");
        assert_eq!(peer_uid(left.as_raw_fd()), Some(unsafe { libc::geteuid() }));
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        assert!(peer_pid(left.as_raw_fd()).is_some_and(|pid| pid > 0));
        assert_eq!(peer_uid(-1), None);
        assert_eq!(peer_pid(-1), None);

        let closed_fd = right.as_raw_fd();
        drop(right);
        assert_eq!(peer_uid(closed_fd), None);
        assert_eq!(peer_pid(closed_fd), None);
        drop(left);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn parses_linux_mem_available() {
        let contents = "MemTotal:       1000 kB\nMemFree: 1 kB\nMemAvailable:    120 kB\n";
        assert_eq!(parse_meminfo(contents), Some((120 * 1024, 1000 * 1024)));
    }
}
