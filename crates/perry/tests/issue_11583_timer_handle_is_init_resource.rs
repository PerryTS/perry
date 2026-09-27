//! #11583: the async_hooks `init` resource of a `setTimeout` / `setImmediate`
//! / `setInterval` is the very handle object the call returned to the program
//! (Node). Perry passed a pointer-tagged raw timer id to `init` and minted a
//! second object for the caller, so `resource === handle` was false.

mod support;

const SOURCE: &str = r#"
import { createHook } from "node:async_hooks";
const seen = new Map<string, object>();
const hook = createHook({
  init(_id: number, type: string, _trigger: number, resource: object) {
    if ((type === "Timeout" || type === "Immediate") && !seen.has(type)) seen.set(type, resource);
  },
});
hook.enable();
const timeout = setTimeout(() => {}, 1);
const immediate = setImmediate(() => {});
const interval = setInterval(() => {}, 1000);
hook.disable();
clearInterval(interval);
console.log(seen.get("Timeout") === timeout, seen.get("Immediate") === immediate);
console.log(typeof (timeout as any).ref, typeof (immediate as any).ref);
"#;

#[test]
fn timer_handles_are_the_resources_init_received() {
    assert_eq!(
        support::compile_and_run(SOURCE),
        "true true\nfunction function\n"
    );
}
