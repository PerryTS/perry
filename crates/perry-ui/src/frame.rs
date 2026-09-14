//! Opt-in desktop window persistence, scoped to the executable and window name.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowState {
    #[default]
    Normal,
    Maximized,
    Fullscreen,
}

/// The normal (unmaximized, non-fullscreen) frame, in backend coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowFrame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub state: WindowState,
}

impl WindowFrame {
    fn valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.x.checked_add(self.width).is_some()
            && self.y.checked_add(self.height).is_some()
    }

    fn decode(value: &str) -> Option<Self> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 7 || fields[0] != "1" {
            return None;
        }
        let frame = Self {
            x: fields[1].parse().ok()?,
            y: fields[2].parse().ok()?,
            width: fields[3].parse().ok()?,
            height: fields[4].parse().ok()?,
            state: match fields[5] {
                "normal" => WindowState::Normal,
                "maximized" => WindowState::Maximized,
                "fullscreen" => WindowState::Fullscreen,
                _ => return None,
            },
        };
        // A terminator detects truncated writes, including a partial state name.
        (fields[6] == "end" && frame.valid()).then_some(frame)
    }
}

// FNV-1a is explicitly fixed so persistence keys survive Rust upgrades.
// The digest is only a filename component, not a security boundary.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn key_for(executable: &Path, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    Some(format!(
        "perry-frame-{:016x}-{:016x}",
        digest(executable.as_os_str().as_encoded_bytes()),
        digest(name.as_bytes())
    ))
}

/// Also used for AppKit's native frame autosave name. Do not key by title:
/// titles can change, and unrelated command-line apps share NSUserDefaults.
pub fn autosave_key(name: &str) -> Option<String> {
    key_for(&std::env::current_exe().ok()?, name)
}

#[derive(Clone, Debug)]
pub struct FrameStore {
    path: PathBuf,
}

impl FrameStore {
    pub fn new(name: &str) -> Option<Self> {
        Self::in_directory(
            &dirs::data_local_dir()?.join("perry").join("window-frames"),
            name,
        )
    }

    /// Use an explicit preferences directory (also useful for isolated tests).
    pub fn in_directory(directory: &Path, name: &str) -> Option<Self> {
        Some(Self {
            path: directory.join(autosave_key(name)?),
        })
    }

    /// Missing, invalid, or unreadable preferences leave launch defaults intact.
    pub fn load(&self) -> Option<WindowFrame> {
        let mut value = String::new();
        std::fs::File::open(&self.path)
            .ok()?
            .take(257)
            .read_to_string(&mut value)
            .ok()?;
        if value.len() > 256 {
            return None;
        }
        WindowFrame::decode(&value)
    }

    /// Replace atomically, including on Windows. A failed write must not
    /// truncate the previous frame or prevent the application from closing.
    pub fn save(&self, frame: WindowFrame) -> std::io::Result<()> {
        if !frame.valid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid window frame",
            ));
        }
        let parent = self.path.parent().expect("frame store has a parent");
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        let state = match frame.state {
            WindowState::Normal => "normal",
            WindowState::Maximized => "maximized",
            WindowState::Fullscreen => "fullscreen",
        };
        writeln!(
            file,
            "1 {} {} {} {} {state} end",
            frame.x, frame.y, frame.width, frame.height
        )?;
        file.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable_isolated_and_cannot_escape_the_store() {
        let exe = Path::new("/apps/editor");
        assert_eq!(digest(b"hello"), 0xa430d84680aabd0b);
        assert_eq!(key_for(exe, ""), None);
        assert_eq!(key_for(exe, "main"), key_for(exe, "main"));
        assert_ne!(key_for(exe, "main"), key_for(exe, "settings"));
        assert_ne!(
            key_for(exe, "main"),
            key_for(Path::new("/apps/other"), "main")
        );
        let key = key_for(exe, "../../settings/窗口").unwrap();
        assert_eq!(Path::new(&key).components().count(), 1);
    }

    #[test]
    fn saves_replace_previous_frames_and_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app").join("main");
        let store = FrameStore { path: path.clone() };
        assert_eq!(store.load(), None);
        for state in [
            WindowState::Normal,
            WindowState::Maximized,
            WindowState::Fullscreen,
        ] {
            let frame = WindowFrame {
                x: -1200,
                y: 40,
                width: 900,
                height: 650,
                state,
            };
            store.save(frame).unwrap();
            assert_eq!(FrameStore { path: path.clone() }.load(), Some(frame));
        }
    }

    #[test]
    fn invalid_preferences_fall_back_without_destroying_a_saved_frame() {
        for value in [
            "",
            "2 0 0 800 600 normal end",
            "1 0 0 800 600 normal",
            "1 0 0 0 600 normal end",
            "1 0 0 800 -600 normal end",
            "1 0 0 800 600 minimized end",
            "1 2147483647 0 800 600 normal end",
            "1 0 0 NaN 600 normal end",
            "1 0 0 800 600 normal end trailing",
        ] {
            assert_eq!(WindowFrame::decode(value), None, "{value}");
        }
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore {
            path: dir.path().join("main"),
        };
        let frame = WindowFrame {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
            state: WindowState::Normal,
        };
        store.save(frame).unwrap();
        assert!(store.save(WindowFrame { width: 0, ..frame }).is_err());
        assert_eq!(store.load(), Some(frame));
        std::fs::write(&store.path, "invalid").unwrap();
        assert_eq!(store.load(), None);
    }
}
