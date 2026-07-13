// #6320: a JSX function component reached as a `Proxy(Component)` value.
//
// `js_jsx`'s `is_valid_closure` probe floored the candidate address at 0x1000 —
// far below `HANDLE_BAND_MAX` (0x100000) — so a proxy's registry id
// (`POINTER_TAG | (PROXY_ID_BAND_START + id)`) reached the `*(addr + 12)`
// CLOSURE_MAGIC read and SIGSEGV'd on unmapped low memory. React calls a
// component through the proxy's [[Call]]; `js_jsx` now does the same.
//
// Not part of the node-compared gap suite (`run_parity_tests.sh` only globs
// `test-files/*.ts`, and node cannot run Perry's server-JSX runtime) — the CI
// gate for this site is the `jsx.rs` unit tests
// (`proxy_wrapped_function_component_dispatches_through_call`,
// `is_valid_closure_rejects_the_handle_band`). This file is the end-to-end
// repro: it must print the three lines below and exit 0.

function Hello(props: any) {
  return <div>hi {props.name}</div>;
}

// No trap: the [[Call]] forwards to the target component.
const Forwarded: any = new Proxy(Hello, {});
console.log(String(<Forwarded name="a" />));

// An `apply` trap intercepts the render and rewrites the props.
const Trapped: any = new Proxy(Hello, {
  apply(target: any, _thisArg: any, args: any[]) {
    return target({ name: "trapped-" + args[0].name });
  },
});
console.log(String(<Trapped name="b" />));

// A proxy of a proxy forwards through both layers.
const Nested: any = new Proxy(new Proxy(Hello, {}), {});
console.log(String(<Nested name="c" />));

console.log("END");
