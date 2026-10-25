Supply `module`, `require` and `exports` in an explicit `.cjs` entry even when it only reads those bindings and has no CommonJS source tokens that trigger wrapping.

Standalone CommonJS parity fixtures use the explicit compile command, so the runner reaches compilation instead of rejecting their filenames as unknown subcommands.
