//! End-to-end test for #4914: `node:cluster` workers actually share a
//! listening port.
//!
//! The canonical cluster pattern — primary forks N workers, each worker
//! `http.createServer().listen(port)` on the SAME port — historically
//! "worked" silently while the second worker's bind failed (EADDRINUSE,
//! swallowed) and no requests were served by it. Workers now bind with
//! SO_REUSEPORT (`NODE_UNIQUE_ID` marks a worker) and report the bound
//! address to the primary over the fork IPC channel, so:
//!  - both workers' binds succeed (the second bind failing would mean no
//!    second `'listening'` event, and this test times out),
//!  - `cluster.on('listening', (worker, address))` fires per worker with
//!    the bound port,
//!  - requests round-trip through the shared port,
//!  - `cluster.on('exit')` fires when the workers are killed.
//!
//! The primary discovers a free port by binding port 0 once and closing it
//! (the `listen(0)` shared-ephemeral-port round-trip itself is #4962).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
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

    let run = Command::new(&output)
        .current_dir(dir)
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

#[cfg(unix)]
#[test]
fn two_workers_share_one_port() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import cluster from "node:cluster";
import http from "node:http";

if (cluster.isPrimary) {
  // Discover a free port; both workers then SO_REUSEPORT-bind it.
  const probe = http.createServer();
  probe.on("listening", () => {
    const port = (probe.address() as any).port;
    probe.close();
    // The probe listener (bound WITHOUT SO_REUSEPORT) is dropped
    // asynchronously by the accept loop; give it a beat so the workers'
    // REUSEPORT binds can't race an EADDRINUSE against it.
    setTimeout(() => start(port), 250);
  });
  probe.listen(0, "127.0.0.1");
} else {
  const port = Number(process.env.CLUSTER_TEST_PORT);
  http
    .createServer((req: any, res: any) => {
      res.end("ok");
    })
    .listen(port, "127.0.0.1");
  // Orphan guard: if the primary dies without killing us, don't squat on
  // the port forever.
  setTimeout(() => process.exit(0), 30000);
}

function start(port: number) {
  const workers: any[] = [];
  let listening = 0;
  let exited = 0;
  let responses = 0;

  // Failure watchdog: exit non-zero with the milestone counters so the
  // harness assert shows where the lifecycle stalled.
  setTimeout(() => {
    console.log("TIMEOUT listening=" + listening + " responses=" + responses + " exited=" + exited);
    process.exit(1);
  }, 25000);

  cluster.on("listening", (worker: any, address: any) => {
    listening++;
    if (address.port !== port) {
      console.log("BAD-PORT got=" + address.port + " want=" + port);
      process.exit(1);
    }
    if (listening === 2) {
      console.log("both-workers-listening true");
      probeWorkers();
    }
  });

  cluster.on("exit", (worker: any, code: any, signal: any) => {
    exited++;
    if (exited === 2) {
      console.log("both-workers-exited true");
      process.exit(0);
    }
  });

  function probeWorkers() {
    for (let i = 0; i < 4; i++) {
      http.get({ host: "127.0.0.1", port: port, path: "/" }, (res: any) => {
        let body = "";
        res.on("data", (c: any) => { body += c; });
        res.on("end", () => {
          if (body === "ok") responses++;
          if (responses === 4) {
            console.log("responses-ok true");
            for (const w of workers) w.kill();
          }
        });
      });
    }
  }

  for (let i = 0; i < 2; i++) {
    workers.push(cluster.fork({ CLUSTER_TEST_PORT: String(port) }));
  }
}
"#,
    );
    assert_eq!(
        stdout,
        "both-workers-listening true\n\
         responses-ok true\n\
         both-workers-exited true\n",
        "cluster workers must both bind the shared port (SO_REUSEPORT), \
         serve requests through it, and report listening/exit to the primary"
    );
}
