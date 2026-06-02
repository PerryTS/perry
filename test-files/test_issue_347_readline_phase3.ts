// Regression test for #347 Phase 3: tty.isatty + process.stdout.columns/rows
// + process.stdin/stdout/stderr.isTTY + process.stdout.on('resize', cb).
// Verifies that:
//   1. `tty.isatty(fd)` lowers to `Expr::TtyIsAtty(arg)` and dispatches
//      to `js_tty_isatty(fd)`.
//   2. `process.stdin.isTTY` / `process.stdout.isTTY` / `process.stderr.isTTY`
//      lower to `Expr::ProcessStdin/Stdout/StderrIsTTY` direct extern
//      calls (libc::isatty on Unix).
//   3. `process.stdout.columns` / `process.stdout.rows` lower to direct
//      extern calls returning the terminal width/height in cells, or
//      `undefined` when stdout isn't a TTY (matching Node).
//   4. `process.stdout.on('resize', cb)` registers a SIGWINCH handler.
//      We can't actually fire SIGWINCH from a piped test, so this
//      checks compile + link only.
//
// All tests run with stdin/stdout piped from /dev/null (CI runner mode).
// In that environment Node returns isTTY=false on every fd; columns/rows
// are undefined; tty.isatty(fd) is false on every fd. Perry matches
// byte-for-byte.

import * as tty from "tty";

// 1. tty.isatty(fd) is false for piped fds (CI runner).
console.log("isatty(0):", tty.isatty(0));
console.log("isatty(1):", tty.isatty(1));
console.log("isatty(2):", tty.isatty(2));

// 2. process.std{in,out,err}.isTTY are false when piped.
console.log("stdin.isTTY:", process.stdin.isTTY === true);
console.log("stdout.isTTY:", process.stdout.isTTY === true);
console.log("stderr.isTTY:", process.stderr.isTTY === true);

// 3. process.stdout.columns / .rows are undefined when not a TTY.
console.log("columns is undefined:", process.stdout.columns === undefined);
console.log("rows is undefined:", process.stdout.rows === undefined);

// 4. process.stdout.on('resize', cb) registration compiles + links.
process.stdout.on("resize", () => {
    console.log("resize fired");
});
console.log("resize handler registered");
