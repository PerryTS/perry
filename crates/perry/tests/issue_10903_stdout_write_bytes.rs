//! Regression coverage for #10903: `process.stdout.write(chunk[, encoding])` /
//! `process.stderr.write(...)` must put a binary chunk (`Buffer`, any
//! `TypedArray`, `DataView`) on the fd byte for byte — exactly the view's
//! window — encode a string chunk with `encoding`, and deliver all of it even
//! when the fd is non-blocking.
//!
//! They used to start from the chunk's display text and ignore `encoding`, so a
//! 4-byte little-endian frame length such as `F7 FF 0F 00` reached the reader
//! as `EF BF BD EF BF BD 0F 00`, `new Uint16Array([0x6968])` printed `26984`,
//! `write("6869", "hex")` wrote four characters, and on a non-blocking pipe an
//! 8 MiB write stopped at the first `EAGAIN` (131,072 bytes delivered).
//!
//! The small expectations are the bytes Node 26.5.1 writes for the same fixture
//! (`PERRY_10903_CASE=<case> node --experimental-strip-types … | xxd -p`),
//! pinned so the test needs no node on the machine. The large ones are
//! recomputed here from the fixture's `pattern()`; their SHA-256 under Node
//! was checked to match when this test was written.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/test_gap_10903_stdout_write_bytes.ts"
));

const CHUNKS_STDOUT: &str = "f7ff0f000af7ff440ac800800ab0b10afefd800afd800a91920aa1a2a30aa2a30a\
feff800afeff800aff800aff990a00000000000004c00a9998970a68c3a96c6c6f0a";
const CHUNKS_STDERR: &str = "f1f20a800aff0a";
const ENCODINGS_STDOUT: &str = "68e90a68e90a68e90a68c3a90a68c3a90af7ff0f000af7ff0f000af7ff0f000a\
6800e9000a006800e9000a00f70af80a";
const ENCODINGS_STDERR: &str = "ff0a800a";
const CALLBACKS_STDOUT: &str = "e00ae10ae20a";
const INTERLEAVE_STDOUT: &str = "6f6e650a800a74776f0a7468726565810a666f75720af50a666976650a";

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> PathBuf {
    let source = dir.join("stdout_write_bytes.ts");
    let binary = dir.join("stdout_write_bytes_bin");
    std::fs::write(&source, SOURCE).expect("write fixture");
    let output = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn run_case(binary: &Path, case: &str) -> (Vec<u8>, Vec<u8>) {
    let output = Command::new(binary)
        .env("PERRY_10903_CASE", case)
        .stdin(Stdio::null())
        .output()
        .expect("run fixture");
    assert!(output.status.success(), "{case}: exit {:?}", output.status);
    (output.stdout, output.stderr)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fixture's `pattern(length, mul)`.
fn pattern(length: usize, mul: usize) -> Vec<u8> {
    (0..length)
        .map(|i| ((i * mul + (i >> 8)) & 0xff) as u8)
        .collect()
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(payload);
    out
}

fn expected_framing() -> Vec<u8> {
    let mut out = frame(&pattern(200, 7));
    out.extend(frame(&pattern(1_048_567, 13)));
    out
}

fn expected_large() -> Vec<u8> {
    let mut out = pattern(5 * 1024 * 1024 + 3, 31);
    out.extend(pattern(3 * 1024 * 1024 + 1, 17));
    out.extend_from_slice(b"done\n");
    out
}

fn expected_many() -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..20_000_usize {
        out.extend_from_slice(&[(i & 0xff) as u8, ((i >> 8) & 0xff) as u8, 0x80, 0x0a]);
        if i % 1000 == 0 {
            out.extend_from_slice(format!("k{i}\n").as_bytes());
        }
    }
    out
}

/// The fixture's `huge` case: one `write()` each of 16, 32 and 64 MiB, every
/// byte `0x80 + MiB` except a leading `MiB` and a trailing `0x0A`. Checked in
/// place — a second 112 MiB buffer to compare against buys nothing.
fn huge_mismatch(got: &[u8]) -> Option<String> {
    let mut at = 0_usize;
    for mib in [16_usize, 32, 64] {
        let len = mib * 1024 * 1024;
        let Some(region) = got.get(at..at + len) else {
            return Some(format!(
                "{mib} MiB chunk: only {} of {len} bytes arrived (total {})",
                got.len().saturating_sub(at),
                got.len()
            ));
        };
        let fill = 0x80 + mib as u8;
        if region[0] != mib as u8 || region[len - 1] != 0x0a {
            return Some(format!("{mib} MiB chunk: wrong first/last byte"));
        }
        if let Some(bad) = region[1..len - 1].iter().position(|b| *b != fill) {
            return Some(format!(
                "{mib} MiB chunk: byte {} is {:#04x}, want {fill:#04x}",
                bad + 1,
                region[bad + 1]
            ));
        }
        at += len;
    }
    (got.len() != at).then(|| format!("{} trailing bytes", got.len() - at))
}

/// Where two byte strings first differ — a multi-megabyte `assert_eq!` dump
/// would bury the answer.
fn first_difference(got: &[u8], want: &[u8]) -> Option<String> {
    if got == want {
        return None;
    }
    let at = got
        .iter()
        .zip(want)
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    let window = |bytes: &[u8]| hex(&bytes[at.min(bytes.len())..(at + 16).min(bytes.len())]);
    Some(format!(
        "got {} bytes, want {}; first difference at {at}: got {} want {}",
        got.len(),
        want.len(),
        window(got),
        window(want)
    ))
}

/// Run `case` with a NON-BLOCKING socket as fd 1 and a reader that dawdles, so
/// the child is guaranteed to see `EAGAIN` many times inside one `write()`.
/// `O_NONBLOCK` lives on the open file description, which the child shares.
#[cfg(unix)]
fn run_case_on_non_blocking_stdout(binary: &Path, case: &str) -> Vec<u8> {
    use std::io::Read;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let (mut ours, theirs) = UnixStream::pair().expect("socketpair");
    theirs.set_nonblocking(true).expect("O_NONBLOCK");
    // The `Command` temporary owns `theirs`; it is dropped at the end of this
    // statement, leaving the child as the only writer so EOF can arrive.
    let mut child = Command::new(binary)
        .env("PERRY_10903_CASE", case)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(theirs)))
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fixture");
    let mut received = Vec::new();
    let mut chunk = vec![0_u8; 16 * 1024];
    loop {
        if received.len() < (1 << 20) {
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        match ours.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => received.extend_from_slice(&chunk[..n]),
            Err(err) => panic!("{case}: read: {err}"),
        }
    }
    assert!(child.wait().expect("wait").success(), "{case}: exit status");
    received
}

#[test]
fn binary_chunks_and_encoded_strings_reach_the_fd_byte_for_byte() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    let mut failures: Vec<String> = Vec::new();
    let mut report = |what: &str, problem: Option<String>| {
        if let Some(problem) = problem {
            failures.push(format!("{what}: {problem}"));
        }
    };
    let unhex = |text: &str| -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
            .collect()
    };

    let (stdout, stderr) = run_case(&binary, "chunks");
    report(
        "chunks stdout",
        first_difference(&stdout, &unhex(CHUNKS_STDOUT)),
    );
    report(
        "chunks stderr",
        first_difference(&stderr, &unhex(CHUNKS_STDERR)),
    );

    let (stdout, stderr) = run_case(&binary, "encodings");
    report(
        "encodings stdout",
        first_difference(&stdout, &unhex(ENCODINGS_STDOUT)),
    );
    report(
        "encodings stderr",
        first_difference(&stderr, &unhex(ENCODINGS_STDERR)),
    );

    // Completion callbacks still fire, after the synchronous return values.
    let (stdout, stderr) = run_case(&binary, "callbacks");
    report(
        "callbacks stdout",
        first_difference(&stdout, &unhex(CALLBACKS_STDOUT)),
    );
    report(
        "callbacks stderr",
        first_difference(&stderr, b"returned true true true\ncb1\ncb2\ncb3\n"),
    );

    // `console.log` and `write` share fd 1 and must stay in program order.
    let (stdout, _) = run_case(&binary, "interleave");
    report(
        "interleave stdout",
        first_difference(&stdout, &unhex(INTERLEAVE_STDOUT)),
    );

    // A view whose ArrayBuffer was transferred away: Node throws a TypeError
    // and writes nothing for it.
    let (stdout, _) = run_case(&binary, "detached");
    report(
        "detached stdout",
        first_difference(&stdout, b"TypeError,TypeError\n"),
    );

    let (stdout, _) = run_case(&binary, "framing");
    report(
        "framing stdout",
        first_difference(&stdout, &expected_framing()),
    );
    let (stdout, _) = run_case(&binary, "large");
    report("large stdout", first_difference(&stdout, &expected_large()));
    let (stdout, _) = run_case(&binary, "many");
    report("many stdout", first_difference(&stdout, &expected_many()));

    // One `write()` each of 16, 32 and 64 MiB.
    let (stdout, _) = run_case(&binary, "huge");
    report("huge stdout", huge_mismatch(&stdout));

    #[cfg(unix)]
    {
        let stdout = run_case_on_non_blocking_stdout(&binary, "large");
        report(
            "large stdout, non-blocking fd",
            first_difference(&stdout, &expected_large()),
        );
        let stdout = run_case_on_non_blocking_stdout(&binary, "framing");
        report(
            "framing stdout, non-blocking fd",
            first_difference(&stdout, &expected_framing()),
        );
    }

    assert!(
        failures.is_empty(),
        "stdout/stderr bytes differ from what Node writes (#10903):\n  {}",
        failures.join("\n  ")
    );
}

/// Undriven, the fixture is the parity sweep's text case: every chunk is
/// printable ASCII on the wire, so it must read exactly as it does under Node.
#[test]
fn undriven_fixture_prints_the_ascii_matrix() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    let output = Command::new(&binary)
        .env_remove("PERRY_10903_CASE")
        .stdin(Stdio::null())
        .output()
        .expect("run undriven fixture");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "ok\nbuffer\nhi!\nu32\nwin\nin\ndv\nslice\nbound\nhex\nb64\nb64url\nlatin1\nign\nret\n\
typeof: boolean\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "err\ne2!\ne3\n");
}
