use super::*;

/// #7261: a `perry_strip_<pid>` directory whose PID is dead must be swept.
/// The control is a pid that is always live on the host platform — init
/// (pid 1) on Unix, System (pid 4) on Windows — proving the sweep
/// discriminates rather than deleting everything it finds. (On Windows
/// pid 1 is never assigned, so it would *correctly* read as dead.)
#[test]
fn sweep_removes_dead_pid_dirs_and_keeps_live_ones() {
    const ALWAYS_LIVE_PID: u32 = if cfg!(windows) { 4 } else { 1 };
    let tmp = std::env::temp_dir();
    let dead = tmp.join(format!("perry_strip_{}", u32::MAX));
    let live = tmp.join(format!("perry_strip_{ALWAYS_LIVE_PID}"));
    let mine = strip_tmp_base().to_path_buf();

    std::fs::create_dir_all(&dead).unwrap();
    std::fs::create_dir_all(&live).ok();
    std::fs::write(dead.join("_x_trimmed.lib"), b"leaked").unwrap();

    sweep_dead_strip_dirs();

    assert!(!dead.exists(), "dead-PID dir must be swept (#7261)");
    assert!(live.exists(), "a LIVE pid's dir must never be touched");
    assert!(mine.exists(), "this process's own dir must never be swept");

    std::fs::remove_dir_all(&live).ok();
}

/// The eight call sites must share one directory, not create eight.
#[test]
fn base_is_created_once_and_stable() {
    let a = strip_tmp_base();
    assert_eq!(a, strip_tmp_base());
    assert!(a.exists());
    assert!(a.ends_with(format!("perry_strip_{}", std::process::id())));
}

/// `u32::MAX as pid_t` is -1, which `kill()` reads as "every process" on
/// Unix; on Windows no such pid is ever assigned. Both platforms must
/// report it dead. Pid 0 is special-cased dead everywhere. The always-live
/// control differs per platform: init is pid 1 on Unix but pid 1 is never
/// assigned on Windows, where the always-running System process is pid 4.
#[test]
fn live_pid_probe_is_correct_at_the_boundaries() {
    assert!(pid_is_live(std::process::id()), "self must read live");
    #[cfg(unix)]
    assert!(pid_is_live(1), "pid 1 (init) must read live on unix");
    #[cfg(windows)]
    assert!(pid_is_live(4), "pid 4 (System) must read live on windows");
    assert!(!pid_is_live(u32::MAX), "an unassignable pid must read dead");
    assert!(!pid_is_live(0), "pid 0 is not an ordinary process");
}
