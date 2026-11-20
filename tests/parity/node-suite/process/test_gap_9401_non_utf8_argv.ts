// #9401: a non-UTF-8 byte in argv must not abort the process.
//
// `std::env::args()` PANICS on an argument that is not valid Unicode, and
// `js_process_argv` collected through it — so `claude -p $'\xff\xfe\x80abc\xc3\x28'`
// died with SIGABRT and a raw Rust backtrace
// (`library/std/src/env.rs: called Result::unwrap() on an Err value`).
// Non-UTF-8 filenames are ordinary on Linux, so this is trivially reachable.
// Node decodes argv leniently: every invalid byte becomes U+FFFD.
//
// The witness re-runs THIS program through `sh`, which is byte-oriented and
// can therefore construct an argument the source file itself cannot contain.
import { spawnSync } from "node:child_process";

const MARKER = "--decode-argv";
const at = process.argv.indexOf(MARKER);

if (at >= 0) {
  const bad = process.argv[at + 1];
  console.log("typeof:", typeof bad);
  console.log("length:", bad.length);
  console.log(
    "codepoints:",
    Array.from(bad)
      .map((c) => c.codePointAt(0)!.toString(16))
      .join(","),
  );
  console.log("utf8-hex:", Buffer.from(bad, "utf8").toString("hex"));
  console.log("next-arg:", process.argv[at + 2]);
} else {
  // \377\376\200 abc \303 \050  — three lone invalid bytes, ASCII "abc", a
  // truncated 2-byte lead byte, then '('.
  const script =
    'exec "$0" "$1" ' +
    MARKER +
    ' "$(printf \'\\377\\376\\200abc\\303\\050\')" tail-marker';
  const child = spawnSync("/bin/sh", ["-c", script, process.argv[0], process.argv[1]], {
    encoding: "utf8",
  });
  process.stdout.write(child.stdout ?? "");
  console.log("child-status:", child.status);
  console.log("child-signal:", child.signal);
}
