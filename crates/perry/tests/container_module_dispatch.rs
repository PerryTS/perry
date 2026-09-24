//! #11211: `perry/container`, `perry/compose` and `perry/workloads` calls must
//! reach perry-stdlib's container FFI.
//!
//! Before the fix HIR lowered every one of these calls to a `NativeMethodCall`
//! that matched no codegen dispatch row, so each evaluated to `undefined` and
//! the container backend was never invoked — the program still compiled,
//! linked and exited 0. So this test asserts on two things that only a live
//! call can produce: the exact JS results, and the exact argv a stub `docker`
//! first on `PATH` recorded. No real container runtime is needed.
//!
//! These modules live behind perry-stdlib's `container` feature, which the
//! prebuilt `full` archive does not include, so the program is compiled with
//! auto-optimize (it rebuilds runtime+stdlib with `container`).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Records its argv (one line per invocation) and prints canned `--format
/// json` output shaped like the docker CLI's.
const STUB_DOCKER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$STUB_LOG"
case "$1" in
  images) echo '{"ID":"sha256:feed","Repository":"alpine","Tag":"3.19","Size":7340032,"CreatedAt":"2026-01-02"}' ;;
  ps) echo '{"ID":"c0ffee01","Names":["web"],"Image":"alpine:3.19","Status":"running","Ports":[],"Labels":{},"CreatedAt":"2026-01-03"}' ;;
  inspect) echo '[{"Id":"c0ffee01","Name":"web","Config":{"Image":"alpine:3.19","Labels":{}},"State":{"Status":"running"},"Created":"2026-01-03"}]' ;;
  run|create) echo "c0ffee02" ;;
  logs) echo "stub-log-line"; echo "stub-err-line" >&2 ;;
  exec) echo "stub-exec-out" ;;
esac
exit 0
"#;

const PROGRAM: &str = r#"
import {
    getBackend, listImages, list, inspect, pullImage, run, stop, remove,
    logs, exec, removeImage, removeIfExists,
} from "perry/container";
import { up, ps, logs as composeLogs, down } from "perry/compose";
import { graph, node, inspectGraph } from "perry/workloads";

async function main(): Promise<void> {
    console.log("backend", getBackend());
    const images = JSON.parse(await listImages());
    console.log("images", images.length, images[0].repository, images[0].tag, images[0].size);
    const all = JSON.parse(await list(true));
    console.log("list", all.length, all[0].id, all[0].name, all[0].status);
    const info = JSON.parse(await inspect("c0ffee01"));
    console.log("inspect", info.id, info.name, info.status, info.image);
    await pullImage("alpine:3.19");
    const h = await run({ image: "alpine:3.19", name: "probe", cmd: ["echo", "hi"] });
    console.log("run", h.id, h.name);
    await stop(h.id, 3);
    await stop(h.id);
    const lg = JSON.parse(await logs(h.id, { tail: 5 }));
    console.log("logs", lg.stdout.trim(), lg.stderr.trim());
    const ex = JSON.parse(await exec(h.id, ["ls", "-la"], { workdir: "/work" }));
    console.log("exec", ex.stdout.trim());
    await remove(h.id, true);
    await remove(h.id);
    await removeImage("alpine:3.19", true);
    console.log("removeIfExists", await removeIfExists("c0ffee01", true));
    const g = graph("g1", { api: JSON.parse(node("api", { image: "alpine:3.19" } as any)) });
    const st = JSON.parse(await inspectGraph(g));
    console.log("inspectGraph", JSON.stringify(st.nodes), st.healthy);
    const stack = await up({ name: "probeproj", services: { web: { image: "alpine:3.19" } } } as any);
    const svc = JSON.parse(await ps(stack));
    console.log("ps", svc.length);
    const cl = JSON.parse(await composeLogs(stack, { service: "web", tail: 2 }));
    console.log("compose logs", cl.stdout.trim());
    await down(stack, { volumes: true });
    console.log("done");
}

main().catch((e) => console.log("error", String(e)));
"#;

const EXPECTED_STDOUT: &str = "\
backend docker
images 1 alpine 3.19 7340032
list 1 c0ffee01 web running
inspect c0ffee01 web running alpine:3.19
run c0ffee02 probe
logs stub-log-line stub-err-line
exec stub-exec-out
removeIfExists true
inspectGraph {\"api\":\"pending\"} true
ps 1
compose logs stub-log-line
done
";

/// The `perry/container` calls, in program order. Each line is one docker
/// invocation; its presence is the liveness proof for that call.
const EXPECTED_CONTAINER_ARGV: &[&str] = &[
    "images --format json",
    "ps --format json --all",
    "inspect --format json c0ffee01",
    "pull alpine:3.19",
    "run --detach --name probe alpine:3.19 echo hi",
    "stop --time 3 c0ffee02",
    "stop c0ffee02",
    "logs --tail 5 c0ffee02",
    "exec --workdir /work c0ffee02 ls -la",
    "rm -f c0ffee02",
    "rm c0ffee02",
    "rmi -f alpine:3.19",
];

fn diagnostics(output: &std::process::Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn container_compose_and_workloads_calls_reach_the_backend() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let stub = bin.join("docker");
    std::fs::write(&stub, STUB_DOCKER).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("main.ts"), PROGRAM).unwrap();

    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(root)
        .args(["compile", "main.ts", "-o", "app"])
        .env_remove("PERRY_NO_AUTO_OPTIMIZE")
        .env("PERRY_NO_CACHE", "1")
        .output()
        .unwrap();
    assert!(compile.status.success(), "{}", diagnostics(&compile));

    let log = root.join("docker-argv.log");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(root.join("app"))
        .current_dir(root)
        .env("PATH", path)
        .env("STUB_LOG", &log)
        // Pin the backend so detection probes only `docker` (resolved via
        // PATH to the stub) whatever else the host has installed.
        .env("PERRY_CONTAINER_BACKEND", "docker")
        .env_remove("PERRY_CONTAINER_VERIFY_IMAGES")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", diagnostics(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        EXPECTED_STDOUT,
        "{}",
        diagnostics(&output)
    );

    let argv = std::fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = argv.lines().collect();
    assert!(
        lines.len() > EXPECTED_CONTAINER_ARGV.len(),
        "the stub docker was barely invoked:\n{argv}"
    );
    assert_eq!(
        &lines[..EXPECTED_CONTAINER_ARGV.len()],
        EXPECTED_CONTAINER_ARGV,
        "full argv log:\n{argv}"
    );

    // removeIfExists / compose / workloads follow. Compose container names
    // carry a random suffix, so match on the stable parts: `up` runs the
    // service with the project label, `logs` reads that container, and
    // `down` force-removes it last.
    let rest = &lines[EXPECTED_CONTAINER_ARGV.len()..];
    let compose_run = rest
        .iter()
        .find(|l| l.starts_with("run --detach --name ") && l.ends_with(" alpine:3.19"))
        .unwrap_or_else(|| panic!("compose up never ran a container:\n{argv}"));
    assert!(
        compose_run.contains("--label perry.compose.project=probeproj")
            && compose_run.contains("--label perry.compose.service=web"),
        "{compose_run}"
    );
    let name = compose_run.split(' ').nth(3).unwrap();
    assert!(
        rest.contains(&format!("logs --tail 2 {name}").as_str()),
        "compose logs did not read {name}:\n{argv}"
    );
    assert_eq!(
        lines.last().copied(),
        Some(format!("rm -f {name}").as_str()),
        "compose down did not remove {name} last:\n{argv}"
    );
}
