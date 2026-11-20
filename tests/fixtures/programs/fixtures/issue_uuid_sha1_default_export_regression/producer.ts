// Regression for the uuid `__perry_wrap_perry_fn_<src>__default` link
// error that PR #903 unmasked.
//
// Pre-#903 the consumer's `import sha1 from "./sha1.js"` and the
// sibling-module `import v35 from "./v35.js"` both registered under
// the literal key `"default"` in the CLI's flat
// `import_function_prefixes` HashMap, so the second insert clobbered
// the first and the consumer-side ExternFuncRef resolved to whichever
// module's prefix landed last. That meant a consumer reading `sha1` as
// a closure value emitted a reference to v35.js's wrapper symbol —
// which exists because v35.js compiles fine. uuid's `v4()` smoke test
// linked OK and "worked" by accident.
//
// #903 corrected the resolution so each default import tracks its own
// source. The collision is gone; consumer-side references now point
// at the correct module's wrapper. That surfaced uuid's preexisting
// `sha1.js` codegen failure (`Uint8Array.of` with 20 args bails out
// in `lower_call.rs:~3226`) as a hard link error
// `Undefined symbols: ___perry_wrap_perry_fn_<sha1>__default`. The
// fix in `crates/perry/src/commands/compile.rs:~5518` extends the
// failed-module stub block to emit closure-wrapper stubs for each
// `Export::Named` so the link succeeds; downstream consumers that
// never invoke the failed module (uuid `v4()` doesn't use sha1) keep
// working, and consumers that DO call in observe a NaN-boxed
// undefined return — same inert shape the existing `__init` stub
// gives the module's top-level init.
//
// Producer shape mirrors PR #890's named-default-decl form. We don't
// embed the actual uuid codegen failure into the fixture (that's a
// separate bug, tracked under `Uint8Array.of` multi-arg lowering);
// the producer's wrapper is real and consumer-side resolution must
// continue to wire through it correctly post-#903.
export default function sha1() {
    return "sha1-ok";
}
