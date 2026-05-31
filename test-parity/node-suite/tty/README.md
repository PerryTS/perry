# node:tty granular parity suite

Focused Node.js-compatible cases for Perry's `node:tty` surface.

These tests avoid requiring a real interactive TTY. They exercise stable CI-safe semantics from Node's tty tests: import shapes, `isatty()` false cases, stdio TTY/dimension shape, constructor export shape, and color-helper behavior.
