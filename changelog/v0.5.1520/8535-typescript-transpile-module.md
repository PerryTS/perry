### Added

- OpenCode Code Mode can use its audited `typescript` runtime surface through
  a native SWC-backed `transpileModule` provider, including TS/TSX lowering,
  compiler enum constants, and diagnostic formatting, without embedding the
  upstream TypeScript compiler or a JavaScript engine.
