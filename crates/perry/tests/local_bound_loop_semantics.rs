use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn compile_and_run_with_timeout(dir: &Path, source: &str, timeout: Duration) -> String {
    let entry = dir.join("timeout.ts");
    let output = dir.join("timeout_bin");
    std::fs::write(&entry, source).expect("write entry");

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

    let mut child = Command::new(&output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled binary");
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout_pipe.read_to_string(&mut output);
        output
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stderr_pipe.read_to_string(&mut output);
        output
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("poll compiled binary") {
            Some(status) => {
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
                return stdout;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                panic!(
                    "captured var loop did not terminate within {timeout:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[test]
fn local_loop_bounds_match_js_trip_counts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function mutatedBound(): number {
  let n = 3;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
    n = 0;
  }
  return count;
}

function fractionalBound(): number {
  let n = 1.5;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

function nanBound(): number {
  let n = 0 / 0;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

function infiniteMutatedBound(): number {
  let n = 1 / 0;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
    n = 0;
  }
  return count;
}

console.log(mutatedBound());
console.log(fractionalBound());
console.log(nanBound());
console.log(infiniteMutatedBound());
"#,
    );
    assert_eq!(stdout, "1\n2\n0\n1\n");
}

#[test]
fn captured_var_counter_uses_one_shared_binding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_with_timeout(
        dir.path(),
        r#"
function unusedCapture(n: any): number {
  for (var k = 0; k < n; k++) {
    const read = () => k;
  }
  return k;
}

function retainedCapture(n: number): string {
  let read = () => -1;
  for (var k = 0; k < n; k++) {
    read = () => k;
  }
  return k + ":" + read();
}

console.log(unusedCapture(5));
console.log(retainedCapture(5));
"#,
        Duration::from_secs(3),
    );
    assert_eq!(stdout, "5\n5:5\n");
}
