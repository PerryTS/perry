//! Regression test for #8748: Sharp's object-form `create` input must produce
//! a real image handle that remains usable through a fluent encode pipeline.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_with_timeout(mut command: Command, timeout: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("run compiled binary");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("poll compiled binary").is_some() {
            return child
                .wait_with_output()
                .expect("collect compiled binary output");
        }
        if start.elapsed() >= timeout {
            child.kill().expect("kill timed out compiled binary");
            let output = child
                .wait_with_output()
                .expect("collect timed out compiled binary output");
            panic!(
                "compiled binary timed out after {timeout:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn sharp_create_encodes_and_decodes_png() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import sharp from "sharp";

const rgb = await sharp({
  create: {
    width: 4,
    height: 3,
    channels: 3,
    background: { r: 1, g: 2, b: 3 },
  },
}).png().toBuffer();
const rgbMetadata = await sharp(rgb).metadata();
console.log(rgbMetadata.format, rgbMetadata.width, rgbMetadata.height, rgbMetadata.channels, rgb.length > 0);

const rgba = await sharp({
  create: {
    width: 2,
    height: 1,
    channels: 4,
    background: { r: 10, g: 20, b: 30, alpha: 0.5 },
  },
}).png().toBuffer();
const rgbaMetadata = await sharp(rgba).metadata();
console.log(rgbaMetadata.format, rgbaMetadata.width, rgbaMetadata.height, rgbaMetadata.channels, rgbaMetadata.hasAlpha);
process.exit(0);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
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

    let mut run_command = Command::new(&output);
    run_command.current_dir(dir.path());
    let run = run_with_timeout(run_command, Duration::from_secs(30));
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "png 4 3 3 true\npng 2 1 4 true\n"
    );
}
