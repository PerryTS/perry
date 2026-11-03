### Runtime

- perf(runtime): `String.prototype.codePointAt` is answered by the native
  string-method dispatch instead of falling through to the primitive-method
  fallback. It had a prototype thunk but no dispatch arm, so every call
  resolved `globalThis.String.prototype.codePointAt`, cloned that closure to
  rebind `this`, and — the thunk not being registered strict — ran `ToObject`
  on the receiver, minting a `String` wrapper with an own index property per
  UTF-16 code unit. Grapheme-aware text measurement calls it once per
  character: on the compiled claude-code TUI it was the only method name
  reaching the fallback, at 99,008 calls and 99,008 wrappers per 400-character
  streamed reply (`PERRY_GC_DIAG=1`, `[gc-primitive-dispatch]`).
