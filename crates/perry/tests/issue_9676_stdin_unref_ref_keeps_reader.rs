//! Regression test for #9676: "TUI input dies after real use".
//!
//! THE DEFECT. On the `process.stdin` OBJECT path — an alias, a parameter, or a
//! destructured field, which is what ink and every TUI built on it use —
//! `unref` was wired to the same `process_stdin_detach_stub` as `pause` and
//! `destroy`. That stub sets a process-global `STDIN_DETACHED` latch, and the
//! runtime's fd-0 reader thread breaks its loop on that latch and EXITS. `ref`
//! was wired to a no-op stub, so nothing ever cleared the latch or restarted
//! the reader.
//!
//! One `unref()`/`ref()` pair therefore left the process with **no reader on fd
//! 0 for the rest of its life**: the event loop kept ticking, the terminal
//! stayed in raw mode, the process still woke on each keystroke — and not one
//! further byte ever reached JS. Ink performs exactly that pair every time its
//! raw-mode refcount drops to zero and comes back, i.e. whenever the last
//! `useInput` component unmounts and a new one mounts. A tool call does that.
//! Hence "input dies after a minute of real use", with the operation that
//! preceded it completing and rendering normally.
//!
//! Node's contract, which this test pins: `ref`/`unref` govern ONLY whether the
//! handle keeps the event loop alive. An unref'd stdin still emits `'data'`.
//!
//! WHY A PTY, AND WHY THIS IS THE ONLY SHAPE THAT CAN FAIL. On a pipe the bug
//! is invisible: perry-stdlib's readline reader owns fd 0 there and never
//! consults the runtime latch, so a pipe-based fixture passes both before and
//! after the fix. The runtime's own reader — the one the latch kills — is the
//! live reader only on a TTY. A test that cannot fail is not a test, so this
//! one runs the child on a real PTY.
//!
//! THE SECOND DEFECT, same family. `rl.close()` (and a literal
//! `process.stdin.pause()`) set perry-stdlib readline's `STDIN_PAUSED`, whose
//! pump branch deliberately leaves `PENDING_DATA` undrained — while readline's
//! fd-0 reader keeps reading and keeps waking the main thread. Only the LITERAL
//! `process.stdin.resume()` spelling cleared that flag; an ALIASED
//! `s.resume()` reached the runtime's object stub, which cleared the runtime's
//! own flags and nothing else. So a TUI that holds stdin in a variable and
//! opens one readline prompt went permanently deaf, with bytes still being
//! consumed off the terminal and CPU still burnt on every keystroke — which is
//! the signature the issue actually recorded.
//!
//! CONTROL. The `none` mode drives the identical keystroke stream with no
//! lifecycle calls at all and must deliver every byte. It passed before the fix
//! too, which is what makes the other two cases' failures attributable to the
//! cycle rather than to the harness, the PTY, or the timing.
//!
//! Measured on `origin/main` (17d00b28e4) before the fix, over a PTY: 2 of 157
//! keystrokes delivered for the unref cycle, 1 of 157 for rl.close + aliased
//! resume, 157 of 157 for the control.

#![cfg(unix)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

/// The child models a TUI's stdin wiring: raw mode, an anonymous handler
/// reached through an ALIASED binding, and a periodic lifecycle cycle chosen by
/// `PERRY_9676_MODE`:
///
///   `none`    — control, no lifecycle calls at all.
///   `unref`   — ink's raw-mode refcount pair, `unref()` then `ref()`.
///   `rlclose` — a readline prompt opened and closed (which pauses stdin, as in
///               Node) and then recovered with an aliased `resume()`.
const SOURCE: &str = r#"
import * as readline from "node:readline";

const s: any = process.stdin;
let rx = 0;
s.setEncoding("utf8");
s.setRawMode(true);
s.addListener("data", (chunk: any) => {
  for (const ch of String(chunk)) {
    rx++;
    console.log("RX:" + rx + ":" + ch);
  }
});
const mode = process.env.PERRY_9676_MODE ?? "none";
// Nothing else in this program touches stdin, so a keystroke that goes missing
// after a cycle went missing because of it.
setInterval(() => {
  if (mode === "unref") {
    s.unref();
    s.ref();
  } else if (mode === "rlclose") {
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
      terminal: false,
    });
    rl.close();
    s.resume();
  }
}, 40);
console.log("READY");
"#;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write PTY fixture");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn open_pty() -> (File, File) {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let rc = unsafe {
        // `null_mut()` for all three: macOS types the trailing termios/winsize
        // params `*mut`, Linux `*const`, and `*mut` coerces to `*const`.
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
    assert!(master >= 0 && slave >= 0);
    // SAFETY: openpty returned two fresh, owned descriptors above.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}

struct PtyChild {
    child: Child,
    input: File,
    lines: Receiver<String>,
}

impl PtyChild {
    fn spawn(program: &Path, mode: &str) -> Self {
        let (master, slave) = open_pty();
        let child_stdin = slave.try_clone().expect("clone PTY slave for stdin");
        let child_stdout = slave.try_clone().expect("clone PTY slave for stdout");
        let mut command = Command::new(program);
        command
            .stdin(Stdio::from(child_stdin))
            .stdout(Stdio::from(child_stdout))
            .stderr(Stdio::null())
            .env("PERRY_9676_MODE", mode);
        // Give the child its own session and make fd 0's PTY its controlling
        // terminal. The stdio descriptors are already installed when this runs.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("spawn PTY child");
        drop(slave);

        let input = master.try_clone().expect("clone PTY master for writes");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(master).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line.trim_end_matches('\r').to_string()).is_err() {
                            break;
                        }
                    }
                    // Linux returns EIO from a PTY master after the slave closes.
                    Err(_) => break,
                }
            }
        });

        let mut session = Self {
            child,
            input,
            lines,
        };
        let ready = session
            .recv_line(Duration::from_secs(30))
            .expect("PTY child never printed READY");
        assert_eq!(ready, "READY", "unexpected first PTY line");
        session
    }

    fn send(&mut self, bytes: &[u8]) {
        self.input.write_all(bytes).expect("write PTY input");
        self.input.flush().expect("flush PTY input");
    }

    fn recv_line(&mut self, timeout: Duration) -> Result<String, RecvTimeoutError> {
        self.lines.recv_timeout(timeout)
    }

    /// Send `letters` one keystroke at a time, waiting for each echo before the
    /// next. Returns what actually came back.
    fn type_and_collect(&mut self, letters: &str) -> String {
        let mut got = String::new();
        for ch in letters.chars() {
            self.send(ch.to_string().as_bytes());
            let deadline = std::time::Instant::now() + Duration::from_millis(1500);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    // Nothing arrived for this keystroke — input is dead. Stop
                    // here rather than burning the rest of the budget; the
                    // assertion below reports how far we got.
                    return got;
                }
                match self.recv_line(remaining) {
                    Ok(line) => {
                        if let Some(rest) = line.strip_prefix("RX:") {
                            if let Some((_, c)) = rest.split_once(':') {
                                got.push_str(c);
                                break;
                            }
                        }
                    }
                    Err(_) => return got,
                }
            }
        }
        got
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Long enough that the 40 ms cycle interval fires many times mid-stream, so a
/// single surviving keystroke after the first cycle cannot pass by luck.
const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz";

#[test]
fn stdin_lifecycle_cycles_keep_delivering_keystrokes() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let program = compile(dir.path());

    // CONTROL: no lifecycle calls at all. This half has always passed; it is
    // here so a failure of the other two cannot be blamed on the PTY, the
    // harness, or timing.
    let mut control = PtyChild::spawn(&program, "none");
    std::thread::sleep(Duration::from_millis(150));
    let control_got = control.type_and_collect(LETTERS);
    assert_eq!(
        control_got, LETTERS,
        "control (no lifecycle calls) lost keystrokes: got {control_got:?} — the \
         harness itself is broken, not the behaviour under test"
    );
    drop(control);

    // GAP 1: `unref()` was the same one-way detach latch as `pause()`/`destroy()`
    // and `ref()` was a no-op stub, so the fd-0 reader exited for good.
    let mut unref = PtyChild::spawn(&program, "unref");
    std::thread::sleep(Duration::from_millis(150));
    let unref_got = unref.type_and_collect(LETTERS);
    assert_eq!(
        unref_got, LETTERS,
        "stdin stopped delivering after an unref()/ref() cycle: got {unref_got:?} of \
         {LETTERS:?}. `unref()` must not stop the fd-0 reader and `ref()` must restore \
         the event-loop hold (#9676)"
    );
    drop(unref);

    // GAP 2: `rl.close()` pauses stdin through perry-stdlib's readline
    // `STDIN_PAUSED`, whose pump branch stops draining `PENDING_DATA` while the
    // reader keeps consuming bytes. Only the LITERAL `process.stdin.resume()`
    // cleared that flag; an aliased `s.resume()` reached the runtime object stub
    // and left stdin permanently deaf.
    let mut rlclose = PtyChild::spawn(&program, "rlclose");
    std::thread::sleep(Duration::from_millis(150));
    let rlclose_got = rlclose.type_and_collect(LETTERS);
    assert_eq!(
        rlclose_got, LETTERS,
        "stdin stopped delivering after rl.close() + an aliased resume(): got \
         {rlclose_got:?} of {LETTERS:?}. The stdin object's pause()/resume() must reach \
         the same flow state as codegen's literal process.stdin spelling (#9676)"
    );
}
