# node:tty granular parity suite

Focused Node.js-compatible cases for Perry's `node:tty` surface.

These tests avoid requiring a real interactive TTY. They exercise stable CI-safe semantics from Node's tty tests: import shapes, `isatty()` false cases, stdio TTY/dimension shape, constructor export shape, and color-helper behavior.

## Optional PTY-backed coverage

Interactive terminal behavior lives outside the default parity suite. Run it explicitly with:

```sh
PERRY_RUN_TTY_INTERACTIVE_TESTS=1 ./test-parity/run_tty_interactive_tests.sh
```

The runner uses `script(1)` to allocate a pseudo-terminal, skips cleanly when `script(1)` or a usable PTY is unavailable, and normalizes terminal control sequences before comparing Node and Perry output. It supports the util-linux `script -q -e -c <cmd> /dev/null` form and the BSD/macOS `script -q /dev/null sh -c <cmd>` form. Use `PERRY_TTY_PTY_COLS` and `PERRY_TTY_PTY_ROWS` to override the default `80x24` PTY size.
