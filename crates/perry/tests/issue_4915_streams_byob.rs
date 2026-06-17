//! Regression test for #4915: Web Streams residuals — BYOB readers and
//! ByteLengthQueuingStrategy were registered surface that threw
//! "not yet implemented". Now `getReader({ mode: "byob" })` /
//! `new ReadableStreamBYOBReader(stream)` mint readers whose `read(view)`
//! fills the caller-supplied buffer, the byte-stream controller exposes
//! `byobRequest` (`view` / `respond(bytesWritten)` / `respondWithNewView`),
//! and queuing strategies do real per-chunk size accounting in
//! `desiredSize` for ReadableStream / WritableStream / TransformStream.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn byob_readers_and_byte_length_strategy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
async function main() {
  // BYOB read(view) drains pre-enqueued byte chunks across chunk
  // boundaries, then reports done with an empty view.
  const rs = new ReadableStream({
    type: "bytes",
    start(controller) {
      controller.enqueue(new Uint8Array([1, 2, 3, 4, 5]));
      controller.enqueue(new Uint8Array([6, 7]));
      controller.close();
    },
  });
  const reader = rs.getReader({ mode: "byob" });
  const a = await reader.read(new Uint8Array(4));
  console.log("a", a.done, Array.from(a.value).join(","));
  const b = await reader.read(new Uint8Array(4));
  console.log("b", b.done, Array.from(b.value).join(","));
  const c = await reader.read(new Uint8Array(4));
  console.log("c", c.done, c.value.byteLength);

  // BYOB mode on a non-byte stream is a TypeError (spec-shaped, so
  // feature detection takes the right path).
  const plain = new ReadableStream({ start(ctl) { ctl.close(); } });
  try {
    plain.getReader({ mode: "byob" });
    console.log("mode", "no-throw");
  } catch (e: any) {
    console.log("mode", e instanceof TypeError);
  }

  // Pull sources write through controller.byobRequest.
  const pulled = new ReadableStream({
    type: "bytes",
    pull(controller) {
      const req = controller.byobRequest;
      if (req !== null && req !== undefined) {
        req.view[0] = 42;
        req.respond(1);
      }
    },
  });
  const r2 = pulled.getReader({ mode: "byob" });
  const d = await r2.read(new Uint8Array(2));
  console.log("d", d.done, Array.from(d.value).join(","));

  // ByteLengthQueuingStrategy: desiredSize counts bytes, not chunks.
  let ctl: any;
  new ReadableStream(
    { start(c) { ctl = c; } },
    new ByteLengthQueuingStrategy({ highWaterMark: 16 }),
  );
  ctl.enqueue(new Uint8Array(6));
  console.log("size", ctl.desiredSize);

  console.log("ok");
}
main();
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "a false 1,2,3,4\nb false 5,6,7\nc true 0\nmode true\nd false 42\nsize 10\nok\n",
        "BYOB read/byobRequest/strategy accounting regressed"
    );
}
