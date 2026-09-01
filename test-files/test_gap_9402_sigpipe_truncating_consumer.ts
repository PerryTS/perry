// #9402: a truncating consumer (`| head -2`, `| grep -q`, `| less` + quit, a
// closed socket) must not kill the process. Node ignores SIGPIPE and lets the
// failing write surface as EPIPE; Perry inherited SIGPIPE's DEFAULT
// disposition, because a compiled program has its own C `main` and never runs
// Rust's `std::rt` startup (which is what installs SIG_IGN for a normal Rust
// binary). Every truncated pipe therefore killed the writer with signal 13.
//
// The witness spawns THIS program again with a marker argument, pipes it into
// `head -2`, and reports the WRITER's exit status (not the pipeline's).
// Pre-fix Perry reports 141 (128 + SIGPIPE); Node reports 0.
import { spawnSync } from "node:child_process";

const MARKER = "--emit-many-lines";

if (process.argv.indexOf(MARKER) >= 0) {
  // Well past a 64 KiB pipe buffer, so `head -2` is guaranteed to have closed
  // the read end long before the last line is written.
  for (let i = 0; i < 50000; i++) {
    console.log("line " + i);
  }
  console.log("writer finished");
} else {
  // `$1` is the script path. Node needs it on the command line; a compiled
  // Perry binary does not, but accepts it as an ignored positional argument —
  // the marker is located with indexOf, not by position, so both runtimes read
  // the same value.
  const script =
    '"$0" "$1" ' + MARKER + ' | head -2 >/dev/null; echo "writer-status=${PIPESTATUS[0]}"';
  const child = spawnSync("/bin/bash", ["-c", script, process.argv[0], process.argv[1]], {
    encoding: "utf8",
  });
  process.stdout.write(child.stdout ?? "");
  console.log("shell-status:", child.status);
}
