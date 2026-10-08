`Function.prototype.call` and `.bind` take their arguments in place. A
built-in body could only receive a variable argument list as a freshly built
rest array, which `call` and `bind` then copied straight back out to forward.
A new rest kind, `FN_REST_NATIVE_ARGS`, passes the caller's argument slice to
a runtime-native body (`JsNativeArgsBody`: callee, receiver, pointer,
length). It is one more arm of the existing rest funnel
(`dispatch_rest_bundled`), so every path that already refuses rest bodies for
direct calls refuses it too. The uncurried `call.bind(WeakMap.prototype.get)`
idiom (call-bound → side-channel → qs) now forwards without an array.

On qs stringify this removes 4.07 M rest arrays (5.96 M → 1.89 M
`js_array_alloc`), 15% of instructions and 140 of 407 copying minors.

`call` holds its receiver, target and arguments in handles only when the
receiver may be boxed or the target is a `this`-capturing method that must be
cloned. Before, a collection during either step could leave the forwarded
arguments stale.

`info_rest` answers bodies without a rest kind with one mask test. The fourth
rest bit would otherwise have added a test to every dynamic closure call.

Tests: `test_gap_call_bind_native_forwarding` and
`test_gap_side_channel_closures` (match Node), and runtime tests asserting
that forwarding through `call` allocates nothing. Re-introducing the array
fails them.
