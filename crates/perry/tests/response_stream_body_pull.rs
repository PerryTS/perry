//! Regression: `new Response(readableStream).text()` must DRIVE the stream's
//! `pull` source, not just snapshot the chunks already queued at construction.
//!
//! Before the fix, `drain_readable_into_bytes` (perry-stdlib streams/subclass.rs)
//! drained only the chunks present at the synchronous `new Response(stream)`
//! instant and force-closed the stream. Data enqueued in `start(controller)`
//! survived (already queued), but anything produced by a `pull(controller)`
//! callback — sync OR async — was dropped, so `.text()` returned `""`. The
//! canonical victim is axios's `trackStream`, which wraps `response.body` in a
//! new stream whose async `pull` does `await reader.read()`; the empty body
//! left the request promise unsettled and hung the app at startup.
//!
//! Fix: the drain now loops `maybe_pull` + the microtask runner until the
//! stream reaches a terminal state, so the (possibly async) pull's
//! `enqueue`/`close` land.

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

    // Run under a wall-clock timeout: this is a regression test for a HANG, so
    // `Command::output()` (which waits forever) would stall CI until the
    // job-level timeout if the bug returns. Poll `try_wait` and kill + fail
    // fast instead. The program's output is a handful of short lines, so the
    // stdout/stderr pipes can't fill before exit.
    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait on compiled binary") {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "compiled binary did not exit within {timeout:?} — the Response stream-body \
                 drain likely regressed to a hang"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut stdout).ok();
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_string(&mut stderr).ok();
    }
    assert!(
        status.success(),
        "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn response_text_drives_stream_pull() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
async function main(): Promise<void> {
  const enc = new TextEncoder();

  // 1. Data enqueued in start() — already queued (worked before the fix).
  const startStream = new ReadableStream({
    start(c: any): void {
      c.enqueue(enc.encode("from-start"));
      c.close();
    },
  });
  console.log("START:" + (await new Response(startStream).text()));

  // 2. Data enqueued by a SYNC pull() — dropped before the fix.
  const syncPull = new ReadableStream({
    pull(c: any): void {
      c.enqueue(enc.encode("from-sync-pull"));
      c.close();
    },
  });
  console.log("SYNC:" + (await new Response(syncPull).text()));

  // 3. Data enqueued by an ASYNC pull(), produced over multiple turns.
  let n = 0;
  const asyncPull = new ReadableStream({
    async pull(c: any): Promise<void> {
      await Promise.resolve();
      n++;
      c.enqueue(enc.encode("p" + n));
      if (n >= 3) c.close();
    },
  });
  console.log("ASYNC:" + (await new Response(asyncPull).text()));

  // 4. The axios trackStream shape: a new stream whose async pull reads from
  //    another stream's reader. This is the exact pattern that hung startup.
  function track(src: any): ReadableStream<Uint8Array> {
    const rd = src.getReader();
    return new ReadableStream({
      async pull(c: any): Promise<void> {
        const r = await rd.read();
        if (r.done) c.close();
        else c.enqueue(r.value);
      },
    });
  }
  const base = new ReadableStream({
    start(c: any): void {
      c.enqueue(enc.encode("tracked-"));
      c.enqueue(enc.encode("body"));
      c.close();
    },
  });
  console.log("TRACK:" + (await new Response(track(base)).text()));

  // 5. arrayBuffer() and json() must drain the pull too.
  const abStream = new ReadableStream({
    async pull(c: any): Promise<void> {
      await Promise.resolve();
      c.enqueue(enc.encode("xyz"));
      c.close();
    },
  });
  const ab = await new Response(abStream).arrayBuffer();
  console.log("ARRBUF:" + new Uint8Array(ab).length);

  const jsonStream = new ReadableStream({
    async pull(c: any): Promise<void> {
      await Promise.resolve();
      c.enqueue(enc.encode('{"k":'));
      c.enqueue(enc.encode("42}"));
      c.close();
    },
  });
  const parsed = await new Response(jsonStream).json();
  console.log("JSON:" + parsed.k);

  // 6. highWaterMark: 0 stream — maybe_pull only pulls a zero-HWM stream when
  //    a read is pending, and the drain has no parked reader, so the drain
  //    must FORCE the pull or this returns empty (CodeRabbit #5776).
  const hwm0 = new ReadableStream(
    {
      pull(c: any): void {
        c.enqueue(enc.encode("hwm0-body"));
        c.close();
      },
    },
    { highWaterMark: 0 },
  );
  console.log("HWM0:" + (await new Response(hwm0).text()));
}
main();
"#,
    );

    assert!(
        out.contains("START:from-start"),
        "start-enqueued body lost: {out}"
    );
    assert!(
        out.contains("SYNC:from-sync-pull"),
        "sync-pull body lost: {out}"
    );
    assert!(out.contains("ASYNC:p1p2p3"), "async-pull body lost: {out}");
    assert!(
        out.contains("TRACK:tracked-body"),
        "trackStream body lost: {out}"
    );
    assert!(
        out.contains("ARRBUF:3"),
        "async-pull arrayBuffer lost: {out}"
    );
    assert!(out.contains("JSON:42"), "async-pull json lost: {out}");
    assert!(
        out.contains("HWM0:hwm0-body"),
        "highWaterMark:0 pull body lost: {out}"
    );
}
