use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::OutputFormat;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct VerboseProgress {
    enabled: bool,
    last_heartbeat: Mutex<Instant>,
}

#[derive(Debug, Default)]
pub(crate) struct ProgressSnapshot<'a> {
    pub stage: &'a str,
    pub module_path: Option<&'a Path>,
    pub module_name: Option<&'a str>,
    pub import_specifier: Option<&'a str>,
    pub api: Option<&'a str>,
    pub visited: Option<usize>,
    pub total: Option<usize>,
    pub collected: Option<usize>,
}

impl VerboseProgress {
    pub(crate) fn new(format: OutputFormat, verbose: u8) -> Self {
        Self {
            enabled: verbose > 0 && matches!(format, OutputFormat::Text),
            last_heartbeat: Mutex::new(Instant::now()),
        }
    }

    pub(crate) fn record(&self, snapshot: ProgressSnapshot<'_>) {
        if self.enabled {
            eprintln!("{}", format_progress_line(&snapshot, false));
        }
    }

    pub(crate) fn heartbeat(&self, snapshot: ProgressSnapshot<'_>) {
        if !self.enabled {
            return;
        }

        let Ok(mut last) = self.last_heartbeat.lock() else {
            return;
        };
        if last.elapsed() >= HEARTBEAT_INTERVAL {
            *last = Instant::now();
            eprintln!("{}", format_progress_line(&snapshot, true));
        }
    }
}

pub(crate) fn format_progress_line(snapshot: &ProgressSnapshot<'_>, heartbeat: bool) -> String {
    let mut out = if heartbeat {
        format!("[progress] heartbeat stage={}", snapshot.stage)
    } else {
        format!("[progress] stage={}", snapshot.stage)
    };

    if let Some(path) = snapshot.module_path {
        out.push_str(" module=");
        out.push_str(&path.display().to_string());
    }
    if let Some(name) = snapshot.module_name {
        out.push_str(" name=");
        out.push_str(name);
    }
    if let Some(import_specifier) = snapshot.import_specifier {
        out.push_str(" import=");
        out.push_str(import_specifier);
    }
    if let Some(api) = snapshot.api {
        out.push_str(" api=");
        out.push_str(api);
    }
    if let Some(visited) = snapshot.visited {
        out.push_str(" visited=");
        out.push_str(&visited.to_string());
        if let Some(total) = snapshot.total {
            out.push('/');
            out.push_str(&total.to_string());
        }
    }
    if let Some(collected) = snapshot.collected {
        out.push_str(" collected=");
        out.push_str(&collected.to_string());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_line_includes_stage_module_import_and_counts() {
        let path = Path::new("/repo/src/main.ts");
        let snapshot = ProgressSnapshot {
            stage: "resolve-import",
            module_path: Some(path),
            module_name: Some("src/main.ts"),
            import_specifier: Some("./dep"),
            api: None,
            visited: Some(7),
            total: Some(12),
            collected: Some(6),
        };

        assert_eq!(
            format_progress_line(&snapshot, false),
            "[progress] stage=resolve-import module=/repo/src/main.ts name=src/main.ts import=./dep visited=7/12 collected=6"
        );
    }

    #[test]
    fn heartbeat_line_is_identifiable_but_keeps_context() {
        let path = Path::new("/repo/src/main.ts");
        let snapshot = ProgressSnapshot {
            stage: "lower",
            module_path: Some(path),
            module_name: None,
            import_specifier: None,
            api: Some("WebAssembly.instantiate"),
            visited: Some(3),
            total: None,
            collected: None,
        };

        assert_eq!(
            format_progress_line(&snapshot, true),
            "[progress] heartbeat stage=lower module=/repo/src/main.ts api=WebAssembly.instantiate visited=3"
        );
    }
}
