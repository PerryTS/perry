// Phase 2B + Phase 2 + #255 fixture: a dispatcher whose methods invoke
// Perry callbacks with JS objects as arguments — exercising the
// trampoline → Perry → js_get_property re-entrancy path that pre-fix
// panicked with "RefCell already borrowed" then "active scope can't
// be dropped".
export function makeDispatcher() {
  return {
    // Single property read inside callback.
    callWithFrame(cb) {
      cb({ deltaTime: 16, frameId: 42 });
    },
    // Multiple property reads + nested method call shape.
    callWithEvent(cb) {
      cb({ type: "click", x: 100, y: 200, target: { id: "btn1" } });
    },
    // Same callback fired twice with different args — exercises the
    // trampoline's Drop guard for REENTRY_SCOPE_PTR (each invocation
    // stashes a fresh scope; the previous one's guard restores null).
    callTwice(cb) {
      cb({ count: 1 });
      cb({ count: 2 });
    },
  };
}
