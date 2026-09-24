//! Small stderr progress display. No template engine or global draw thread.
//! A ticker exists only while an interactive, unknown-length transfer runs.
use std::{
    fmt,
    io::{IsTerminal, Write},
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct State {
    length: Option<u64>,
    position: u64,
    message: String,
    bytes: bool,
    visible: bool,
    color: bool,
    finished: bool,
    frame: usize,
    last: Instant,
    start: Instant,
}
pub struct ProgressBar {
    shared: Arc<(Mutex<State>, Condvar)>,
    ticker: Mutex<Option<JoinHandle<()>>>,
}
impl ProgressBar {
    pub fn new(length: u64) -> Self {
        Self::create(Some(length), false, false, std::io::stderr().is_terminal())
    }
    pub fn download(length: Option<u64>, color: bool) -> Self {
        Self::create(length, true, color, std::io::stderr().is_terminal())
    }
    pub fn hidden() -> Self {
        Self::create(None, false, false, false)
    }
    fn create(length: Option<u64>, bytes: bool, color: bool, visible: bool) -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(State {
                    length,
                    position: 0,
                    message: String::new(),
                    bytes,
                    visible,
                    color: color && std::env::var_os("NO_COLOR").is_none(),
                    finished: false,
                    frame: 0,
                    last: Instant::now() - Duration::from_secs(1),
                    start: Instant::now(),
                }),
                Condvar::new(),
            )),
            ticker: Mutex::new(None),
        }
    }
    fn update(&self, f: impl FnOnce(&mut State)) {
        let mut state = self.shared.0.lock().unwrap();
        f(&mut state);
        draw(&mut state, false);
    }
    pub fn set_message(&self, message: impl Into<String>) {
        self.update(|s| s.message = message.into());
    }
    pub fn set_position(&self, position: u64) {
        self.update(|s| s.position = position);
    }
    pub fn inc(&self, amount: u64) {
        self.update(|s| s.position = s.position.saturating_add(amount));
    }
    pub fn set_length(&self, length: u64) {
        self.update(|s| s.length = Some(length));
    }
    pub fn position(&self) -> u64 {
        self.shared.0.lock().unwrap().position
    }
    pub fn elapsed(&self) -> Duration {
        self.shared.0.lock().unwrap().start.elapsed()
    }
    pub fn enable_steady_tick(&self, interval: Duration) {
        if !self.shared.0.lock().unwrap().visible {
            return;
        }
        let mut ticker = self.ticker.lock().unwrap();
        if ticker.is_some() {
            return;
        }
        let shared = self.shared.clone();
        *ticker = Some(std::thread::spawn(move || {
            let mut state = shared.0.lock().unwrap();
            while !state.finished {
                draw(&mut state, true);
                state = shared
                    .1
                    .wait_timeout(state, interval.max(Duration::from_millis(50)))
                    .unwrap()
                    .0;
            }
        }));
    }
    pub fn abandon_with_message(&self, message: impl Into<String>) {
        self.finish(Some(message.into()));
    }
    pub fn println(&self, message: impl AsRef<str>) {
        let mut state = self.shared.0.lock().unwrap();
        let mut stderr = std::io::stderr().lock();
        if state.visible {
            let _ = write!(stderr, "\r\x1b[2K");
        }
        let _ = writeln!(stderr, "{}", message.as_ref());
        drop(stderr);
        draw(&mut state, true);
    }
    pub fn finish_and_clear(&self) {
        self.finish(None);
    }
    pub fn finish_with_message(&self, message: impl Into<String>) {
        self.finish(Some(message.into()));
    }
    fn finish(&self, message: Option<String>) {
        let mut state = self.shared.0.lock().unwrap();
        if state.finished {
            return;
        }
        state.finished = true;
        if state.visible {
            let mut stderr = std::io::stderr().lock();
            let _ = write!(stderr, "\r\x1b[2K");
            if let Some(message) = message {
                let _ = writeln!(stderr, "  {message}");
            }
            let _ = stderr.flush();
        }
        self.shared.1.notify_all();
    }
}
impl Drop for ProgressBar {
    fn drop(&mut self) {
        self.finish_and_clear();
        if let Some(thread) = self.ticker.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}
fn draw(state: &mut State, force: bool) {
    if !state.visible
        || state.finished
        || (!force && state.last.elapsed() < Duration::from_millis(100))
    {
        return;
    }
    state.last = Instant::now();
    state.frame += 1;
    let line = render(state);
    let width = usize::from(console::Term::stderr().size().1)
        .saturating_sub(1)
        .max(1);
    let clipped = console::truncate_str(&line, width, "…");
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K{clipped}");
    let _ = stderr.flush();
}
fn render(state: &State) -> String {
    let spinner = ['|', '/', '-', '\\'][state.frame % 4];
    let mut line = format!("  {spinner} ");
    if let Some(total) = state.length {
        let filled = if total == 0 {
            0
        } else {
            ((u128::from(state.position.min(total)) * 30) / u128::from(total)) as usize
        };
        if state.color {
            line.push_str("\x1b[36m");
        }
        line.push('[');
        line.push_str(&"=".repeat(filled));
        line.push_str(&"-".repeat(30 - filled));
        line.push_str("] ");
        if state.color {
            line.push_str("\x1b[0m");
        }
    }
    if state.bytes {
        let rate = state.position as f64 / state.start.elapsed().as_secs_f64().max(0.001);
        line.push_str(&HumanBytes(state.position).to_string());
        if let Some(total) = state.length {
            line.push_str(&format!("/{}", HumanBytes(total)));
        }
        line.push_str(&format!(" ({}/s", HumanBytes(rate as u64)));
        if let Some(total) = state.length.filter(|_| rate > 0.0) {
            let seconds =
                (total.saturating_sub(state.position) as f64 / rate).min(u64::MAX as f64) as u64;
            line.push_str(&format!(
                ", eta {}",
                HumanDuration(Duration::from_secs(seconds))
            ));
        }
        line.push(')');
    } else {
        line.push_str(&state.message);
    }
    line
}
pub struct HumanBytes(pub u64);
impl fmt::Display for HumanBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = self.0 as f64;
        let mut index = 0;
        let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
        while value >= 1024.0 && index < units.len() - 1 {
            value /= 1024.0;
            index += 1;
        }
        if index == 0 {
            write!(f, "{} B", self.0)
        } else {
            write!(f, "{value:.2} {}", units[index])
        }
    }
}
pub struct HumanDuration(pub Duration);
impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0.as_secs();
        if s < 60 {
            write!(f, "{s}s")
        } else if s < 3600 {
            write!(f, "{}m {}s", s / 60, s % 60)
        } else {
            write!(f, "{}h {}m", s / 3600, s / 60 % 60)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_progress_tracks_bytes_without_a_ticker() {
        let bar = ProgressBar::hidden();
        bar.inc(42);
        bar.enable_steady_tick(Duration::from_millis(1));
        assert_eq!(bar.position(), 42);
        assert!(bar.ticker.lock().unwrap().is_none());
        bar.finish_and_clear();
        assert!(bar.shared.0.lock().unwrap().finished);
    }
    #[test]
    fn no_color_and_zero_length() {
        let bar = ProgressBar::download(Some(0), false);
        let state = bar.shared.0.lock().unwrap();
        let line = render(&state);
        assert!(!line.contains('\x1b'));
        assert!(line.contains("0 B/0 B"));
    }
}
