The GC root-dominance symbol scan accepts the `C-unwind` ABI, restoring the
`gc-root-dominance` and `gc-root-dominance-statepoints` gates. Both were red on
main with `POLL_CAPABLE_RUNTIME entries that name no runtime symbol:
js_closure_call2`.

The entry was correct. `js_closure_call2` is a real, exported runtime symbol;
what the scan could not see was its ABI string, because `runtime_symbols`
matched `extern "C" fn js_*` and the function is declared `extern "C-unwind"`.
The audit's own message warns against the wrong repair ("do not just delete it,
or the audit goes green and the hole stays"), which is exactly the trap: the
name was never wrong.

`"C-unwind"` is a distinct ABI string but the same exported C symbol, and the
runtime uses it for every entry point a JS exception may unwind through — today
18 symbols, including `js_throw`, the whole `js_native_call_method*` dispatch
family, the `js_typed_feedback_native_call_*` paths, and `js_closure_call2`.
Those are the allocating, poll-capable calls the analysis exists to reason
about, so every consumer of `runtime_symbols` had been under-counting them; the
phantom entry was the symptom that made a wider blindness visible.

The scan now accepts both spellings, taking `runtime_symbols` from 3803 to 3821
exported symbols.
