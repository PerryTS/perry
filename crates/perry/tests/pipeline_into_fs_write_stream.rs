//! Regression: `stream.pipeline(source, fs.createWriteStream(path), cb)` must
//! treat the fs write stream as a stream. Its object carries none of the
//! hidden classic-stream flags the pipeline checked, so it was classified as an
//! options object and the call threw ERR_INVALID_ARG_TYPE ("body"). Uploading
//! through `pipeline(passThrough, fs.createWriteStream(path))` therefore
//! failed, and the orphaned write stream then died on an unhandled ENOENT.
//!
//! Past classification the pipeline also listened for the wrong completion
//! event (`end` instead of `finish`) and ended the destination through
//! classic-stream state the fs stream does not have, so the callback would
//! never have run and the file never closed.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.js");
    std::fs::write(&entry, source).expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn promisified_pipeline_writes_a_passthrough_into_an_fs_write_stream() {
    let stdout = compile_and_run(
        "const fs = require('fs');\n\
         const os = require('os');\n\
         const path = require('path');\n\
         const stream = require('stream');\n\
         const { promisify } = require('util');\n\
         const pipeline = promisify(stream.pipeline);\n\
         const file = path.join(os.tmpdir(), 'perry-pipeline-fs-' + process.pid + '.bin');\n\
         const body = new stream.PassThrough();\n\
         const done = pipeline(body, fs.createWriteStream(file));\n\
         body.write('hello ');\n\
         body.write('world');\n\
         body.end();\n\
         done.then(() => {\n\
           console.log('done', fs.readFileSync(file, 'utf8'));\n\
           fs.unlinkSync(file);\n\
         }, (err) => console.log('failed', err && err.code, err && err.message));\n",
    );
    assert_eq!(stdout, "done hello world\n");
}
