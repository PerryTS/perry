//! Which package manager, if any, owns this `perry` binary.
//!
//! `perry update` replaces the running executable in place. That is right for
//! a tarball or `install.sh` install, and wrong for every managed one: a
//! Homebrew formula, an npm package, a `.deb` and a winget package each keep
//! their own record of what is installed and what version it is. Overwriting
//! the file underneath them leaves that record lying, so the next
//! `brew upgrade` or `npm install -g` either reinstalls over the top or
//! reports a version that is not what is on disk.
//!
//! The npm case is the worst of them, because Perry is published as a wrapper
//! package plus a per-platform binary package. Replacing the binary desyncs it
//! from the wrapper that launched it.
//!
//! So the rule is: detect the owner, and when there is one, tell the user the
//! command that owner understands rather than doing it for them.
//!
//! # Failing open
//!
//! Every heuristic here answers "is this definitely managed?", never "is this
//! definitely unmanaged?". An unrecognised layout resolves to
//! [`InstallChannel::SelfManaged`], which is the permissive answer. Getting
//! that wrong costs an in-place update on a machine that could have used a
//! package manager; getting the opposite wrong would refuse to self-update a
//! plain tarball install, which is the majority case and the one with no
//! alternative path.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallChannel {
    /// A tarball, `install.sh`, or a locally built binary. Ours to replace.
    SelfManaged,
    Homebrew,
    Npm,
    Apt,
    Winget,
}

impl InstallChannel {
    /// What the user should run instead, when this channel owns the binary.
    pub(crate) fn upgrade_command(self) -> Option<&'static str> {
        match self {
            Self::SelfManaged => None,
            Self::Homebrew => Some("brew upgrade perryts/perry/perry"),
            Self::Npm => Some("npm install -g @perryts/perry@latest"),
            Self::Apt => Some("sudo apt update && sudo apt install --only-upgrade perry"),
            Self::Winget => Some("winget upgrade PerryTS.Perry"),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SelfManaged => "self-managed",
            Self::Homebrew => "Homebrew",
            Self::Npm => "npm",
            Self::Apt => "apt",
            Self::Winget => "winget",
        }
    }

    /// The extra sentence worth saying for channels where "just run the other
    /// command" undersells why we refused.
    pub(crate) fn refusal_detail(self) -> Option<&'static str> {
        match self {
            Self::Npm => Some(
                "replacing the binary would also desync it from the \
                 @perryts/perry wrapper package that launched it",
            ),
            _ => None,
        }
    }
}

/// Classify the running binary's install channel.
pub(crate) fn detect() -> InstallChannel {
    let Ok(exe) = std::env::current_exe() else {
        return InstallChannel::SelfManaged;
    };
    // Resolve symlinks BEFORE classifying. Homebrew's `perry` in
    // `/usr/local/bin` is a symlink into the Cellar, and `install.sh` may
    // leave one too — classifying the link rather than its target would miss
    // every Homebrew install there is.
    let resolved = std::fs::canonicalize(&exe).unwrap_or(exe);
    classify(&resolved, dpkg_owns_perry())
}

/// Does dpkg have a file list for a `perry` package?
///
/// A plain existence check rather than shelling out to `dpkg -S`: this runs on
/// the update path of every command, so it must not spawn a process, and it
/// must not fail noisily in a sandbox that has no `dpkg` on `PATH`.
fn dpkg_owns_perry() -> bool {
    cfg!(target_os = "linux") && Path::new("/var/lib/dpkg/info/perry.list").exists()
}

/// The classification itself, with the filesystem answer passed in so the
/// whole table is testable without one.
pub(crate) fn classify(exe: &Path, dpkg_owns: bool) -> InstallChannel {
    // Split on BOTH separators rather than using `Path::components`, which is
    // platform-dependent: a Windows path handed to a Unix build comes back as
    // one component, so the winget table below could only ever be exercised on
    // Windows. The rules here are about names in the path, not about path
    // semantics, so a uniform split is both simpler and testable everywhere.
    let text = exe.to_string_lossy().replace('\\', "/");
    let components: Vec<&str> = text.split('/').filter(|c| !c.is_empty()).collect();
    let has = |name: &str| components.iter().any(|c| *c == name);

    // Homebrew: everything lives under a Cellar, whatever the prefix is
    // (`/opt/homebrew` on Apple silicon, `/usr/local` on Intel,
    // `/home/linuxbrew/.linuxbrew` on Linux).
    if has("Cellar") {
        return InstallChannel::Homebrew;
    }

    // npm: a global install lands in `<prefix>/lib/node_modules/...`, and nvm,
    // pnpm and a project-local install all keep the same component. Perry's
    // launcher execs the platform binary out of an optional dependency, so the
    // running executable is inside `node_modules` in every one of those.
    if has("node_modules") {
        return InstallChannel::Npm;
    }

    // winget puts portable packages under its own Packages directory, and
    // store-delivered ones under WindowsApps.
    if has("WindowsApps") || has("WinGet") {
        return InstallChannel::Winget;
    }

    // apt: dpkg owns `/usr/bin` and `/usr/lib`, and specifically does NOT own
    // `/usr/local`, which is where `install.sh` puts things. Both halves are
    // required — the path alone would misclassify a hand-placed binary, and
    // the dpkg list alone would claim a tarball install on a machine that
    // happens to also have the package installed elsewhere.
    let under_usr = text.starts_with("/usr/bin/") || text.starts_with("/usr/lib/");
    if dpkg_owns && under_usr {
        return InstallChannel::Apt;
    }

    InstallChannel::SelfManaged
}

/// Is the directory holding the binary writable by this process?
///
/// `install.sh` installs into `/usr/local/bin`, which is root-owned on a
/// default macOS and most Linux boxes. Discovering that only when the install
/// tries to rename over the executable means failing halfway through, so this
/// is checked before anything is downloaded.
pub(crate) fn install_dir_is_writable() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent().map(PathBuf::from) else {
        return false;
    };
    let probe = dir.join(".perry-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Running under `sudo`, i.e. this process's `$HOME` may not be the invoking
/// user's.
///
/// Writing the update cache here would leave a root-owned file in that user's
/// `~/.perry`, and every later non-root run would fail to update it — the
/// check would then re-run on every invocation forever.
pub(crate) fn running_via_sudo() -> bool {
    std::env::var_os("SUDO_USER").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_of(path: &str, dpkg_owns: bool) -> InstallChannel {
        classify(Path::new(path), dpkg_owns)
    }

    #[test]
    fn homebrew_is_detected_under_every_prefix() {
        for path in [
            "/opt/homebrew/Cellar/perry/1.0/bin/perry",
            "/usr/local/Cellar/perry/1.0/bin/perry",
            "/home/linuxbrew/.linuxbrew/Cellar/perry/1.0/bin/perry",
        ] {
            assert_eq!(channel_of(path, false), InstallChannel::Homebrew, "{path}");
        }
    }

    /// The launcher execs the platform binary out of an optional dependency,
    /// so the running executable is inside `node_modules` for a global
    /// install, an nvm install and a project-local one alike.
    #[test]
    fn npm_is_detected_wherever_node_modules_appears() {
        for path in [
            "/usr/local/lib/node_modules/@perryts/perry-darwin-arm64/bin/perry",
            "/home/u/.nvm/versions/node/v26.5.1/lib/node_modules/@perryts/perry/bin/perry",
            "/home/u/project/node_modules/@perryts/perry-linux-x64/bin/perry",
        ] {
            assert_eq!(channel_of(path, false), InstallChannel::Npm, "{path}");
        }
    }

    /// Both halves are required. `/usr/local` is where `install.sh` puts
    /// things and dpkg never owns it, so a machine with the .deb installed
    /// elsewhere must not have its tarball binary claimed by apt.
    #[test]
    fn apt_needs_both_a_dpkg_list_and_a_dpkg_owned_path() {
        assert_eq!(channel_of("/usr/bin/perry", true), InstallChannel::Apt);
        assert_eq!(
            channel_of("/usr/bin/perry", false),
            InstallChannel::SelfManaged,
            "no dpkg list means no apt package, whatever the path"
        );
        assert_eq!(
            channel_of("/usr/local/bin/perry", true),
            InstallChannel::SelfManaged,
            "dpkg does not own /usr/local — that is install.sh's directory"
        );
    }

    #[test]
    fn winget_is_detected_for_both_delivery_shapes() {
        assert_eq!(
            channel_of(
                r"C:\Program Files\WindowsApps\PerryTS.Perry_1.0\perry.exe",
                false
            ),
            InstallChannel::Winget
        );
        assert_eq!(
            channel_of(
                r"C:\Users\u\AppData\Local\Microsoft\WinGet\Packages\PerryTS.Perry_x\perry.exe",
                false
            ),
            InstallChannel::Winget
        );
    }

    /// The permissive default. An unrecognised layout must be treated as ours
    /// to replace, because refusing to self-update a tarball install would
    /// break the majority case — the one with no other upgrade path.
    #[test]
    fn an_unrecognized_layout_fails_open_to_self_managed() {
        for path in [
            "/usr/local/bin/perry",
            "/home/u/tools/perry",
            "/home/u/perry/target/release/perry",
            "/opt/perry/bin/perry",
        ] {
            assert_eq!(
                channel_of(path, false),
                InstallChannel::SelfManaged,
                "{path}"
            );
        }
    }

    /// Every managed channel must be able to tell the user what to run
    /// instead. A refusal with no alternative is a dead end.
    #[test]
    fn every_managed_channel_offers_a_command_and_self_managed_does_not() {
        for channel in [
            InstallChannel::Homebrew,
            InstallChannel::Npm,
            InstallChannel::Apt,
            InstallChannel::Winget,
        ] {
            let command = channel
                .upgrade_command()
                .unwrap_or_else(|| panic!("{} must offer an upgrade command", channel.label()));
            assert!(!command.is_empty());
        }
        assert_eq!(InstallChannel::SelfManaged.upgrade_command(), None);
    }
}
