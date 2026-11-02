### Added

- Added the native `Bun.Transpiler` and in-memory `Bun.build` subsets used by
  runtime hook modules, including TypeScript/JSX lowering, import scanning,
  external modules, and `onResolve`/`onLoad` plugin hooks. Build failures now
  return useful filename and source-position logs instead of terminating the
  process. (#9602)
