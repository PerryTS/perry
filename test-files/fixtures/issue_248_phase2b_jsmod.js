// Phase 2B fixture: a tiny dispatcher object whose methods take Perry
// closures and call them with various arities. Using methods (not
// free functions) forces the perry-jsruntime `js_call_method` FFI to
// be linked (no perry-runtime weak stub to shadow it), so V8 actually
// runs in the regression test.
export function makeDispatcher() {
  return {
    call0(cb) { cb(); },
    call1(cb) { cb(42); },
    call2(cb) { cb(10, 20); },
    call3(cb) { cb(1, 2, 3); },
    callTwice(cb) { cb(); cb(); },
  };
}
