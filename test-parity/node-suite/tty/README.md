# node:tty granular parity suite

Focused Node.js/Deno-compatible cases for Perry's `node:tty` surface.

These tests avoid requiring a real interactive TTY. They exercise stable CI-safe semantics from Node's tty tests and Deno's `tests/unit_node/tty_test.ts`: import shapes, `isatty()` false cases, stdio TTY/dimension shape, and constructor export shape.
