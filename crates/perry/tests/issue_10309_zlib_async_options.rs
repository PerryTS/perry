//! Async zlib overloads preserve options in direct and captured calls.
use std::process::Command;

#[test]
fn async_zlib_options_reach_codecs_and_preserve_callback_contract() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let entry = dir.path().join("zlib_async_options.ts");
    let binary = dir.path().join("zlib_async_options");
    std::fs::write(
        &entry,
        include_str!("../../../test-files/test_gap_10309_zlib_async_options.ts"),
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile regression fixture");
    assert!(
        compile.status.success(),
        "compile failed:\n{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&binary)
        .output()
        .expect("run regression fixture");
    assert!(
        run.status.success(),
        "run failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        r#"gzip two true true
gzip options true true
gzip undefined true true
gzip null true true
gzip callbacks 4
deflate two true true
deflate options true true
deflate undefined true true
deflate null true true
deflate callbacks 4
deflateRaw two true true
deflateRaw options true true
deflateRaw undefined true true
deflateRaw null true true
deflateRaw callbacks 4
gunzip two true true
gunzip options true true
gunzip undefined true true
gunzip null true true
gunzip callbacks 4
inflate two true true
inflate options true true
inflate undefined true true
inflate null true true
inflate callbacks 4
inflateRaw two true true
inflateRaw options true true
inflateRaw undefined true true
inflateRaw null true true
inflateRaw callbacks 4
unzip two true true
unzip options true true
unzip undefined true true
unzip null true true
unzip callbacks 4
brotliCompress two true true
brotliCompress options true true
brotliCompress undefined true true
brotliCompress null true true
brotliCompress callbacks 4
brotliDecompress two true true
brotliDecompress options true true
brotliDecompress undefined true true
brotliDecompress null true true
brotliDecompress callbacks 4
zstdCompress two true true
zstdCompress options true true
zstdCompress undefined true true
zstdCompress null true true
zstdCompress callbacks 4
zstdDecompress two true true
zstdDecompress options true true
zstdDecompress undefined true true
zstdDecompress null true true
zstdDecompress callbacks 4
direct stored true
direct brotli true
gzip levels true true true
gzip invalid level ERR_OUT_OF_RANGE
deflate levels true true true
deflate invalid level ERR_OUT_OF_RANGE
deflateRaw levels true true true
deflateRaw invalid level ERR_OUT_OF_RANGE
invalid callback ERR_INVALID_ARG_TYPE
invalid callback ERR_INVALID_ARG_TYPE
invalid callback ERR_INVALID_ARG_TYPE
invalid callback ERR_INVALID_ARG_TYPE
second callback true true
"#,
    );
}
