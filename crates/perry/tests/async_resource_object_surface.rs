//! #10926 / honest-tags row 13 — `AsyncResource` and `AsyncHook` are ordinary
//! objects, and a subclass inherits through the prototype chain.
//!
//! Two bugs, one representation. The DIRECT path
//! (`new AsyncResource(...)`, `createHook(...)`) handed JS a raw
//! `Box::into_raw` address with no `GcHeader`, so `JSON.stringify` dispatched
//! on whatever bytes preceded the `Box` and answered `""` (#10926). The
//! SUBCLASS path compensated for a missing prototype link by copying five
//! methods onto every instance as own properties and stashing that same
//! header-less `Box` in a user-visible `__perryAsyncResourceBacking` field —
//! so `Object.keys(new R())` leaked it and `JSON.stringify` serialised the
//! header-less value.
//!
//! The leak was the symptom. The cause is that
//! `Object.getPrototypeOf(R.prototype) === AsyncResource.prototype` is false:
//! `reserved_native_parent_prototype_bits` wires `EventEmitter` and
//! `EventEmitterAsyncResource` and simply never got an `AsyncResource` arm.
//! Linking the chain deletes the copies and the field.
//!
//! THIS HALF fixes the DIRECT path only: `new AsyncResource(...)` and
//! `createHook(...)` become ordinary objects, so `direct-json`, `hook-json`
//! and `nested` match node. The SUBCLASS half is HELD: linking
//! `R.prototype.[[Prototype]]` to `AsyncResource.prototype` needs a codegen
//! condition in `perry-codegen/src/expr/property_get.rs:1351`, which matches
//! `class_name == "AsyncResource"` exactly and so misses a subclass; widening
//! it belongs to the lane already restructuring that file. Until then
//! `sub-keys`, `sub-gopn`, `sub-json` and `proto-chain` keep their CURRENT
//! (node-divergent) values here, marked below, so this test states the truth
//! rather than an aspiration.
//!
//! MUST-FAIL: committed BEFORE the fix. On v0.5.1633 the three direct lines
//! differ; with this half they match node 26.8.1.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

const PROGRAM: &str = r#"
import { AsyncResource, createHook } from "node:async_hooks";
class MyRes extends AsyncResource { constructor() { super("MYRES"); } }
const AR: any = AsyncResource;
const sub: any = new MyRes();
const direct: any = new AsyncResource("DIRECT");
const hook: any = createHook({ init() {} });
console.log("sub-keys", JSON.stringify(Object.keys(sub)));
console.log("sub-gopn", JSON.stringify(Object.getOwnPropertyNames(sub).sort()));
console.log("sub-json", JSON.stringify(sub));
console.log("direct-keys", JSON.stringify(Object.keys(direct)));
console.log("direct-json", JSON.stringify(direct));
console.log("hook-json", JSON.stringify(hook));
console.log("nested", JSON.stringify({ a: direct, b: hook }));
console.log("proto-chain", Object.getPrototypeOf(MyRes.prototype) === AR.prototype);
console.log("instanceof", sub instanceof AsyncResource, sub instanceof MyRes, direct instanceof AsyncResource);
console.log("typeof", typeof sub, typeof direct, typeof hook);
console.log("asyncId", typeof sub.asyncId(), typeof direct.asyncId());
console.log("trigger", typeof sub.triggerAsyncId());
let ran = 0;
sub.runInAsyncScope(() => { ran++; });
direct.runInAsyncScope(() => { ran++; });
console.log("runInAsyncScope", ran);
console.log("bind", typeof sub.bind(() => 1));
hook.enable(); hook.disable();
console.log("hook-methods", typeof hook.enable, typeof hook.disable);
console.log("identity", sub === sub, direct === new AsyncResource("OTHER"));
const m = new Map<any, number>([[direct, 1], [hook, 2]]);
console.log("map", m.size, m.get(direct), m.get(hook));
"#;

/// Every line is node 26.8.1's. Seven differ on v0.5.1633:
/// `sub-keys`, `sub-gopn`, `sub-json`, `direct-json`, `hook-json`, `nested`
/// and `proto-chain`. The other ten are already correct and are pinned here so
/// the representation change cannot quietly break them — `instanceof`,
/// `typeof`, the method results, identity and `Map` keys all have to survive.
#[test]
fn async_resource_and_hook_match_nodes_object_surface() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(dir.path(), PROGRAM);
    assert_eq!(
        stdout,
        // The first three lines and `proto-chain` are HELD (subclass half,
        // pending the codegen widening at `property_get.rs:1351`) -- node
        // gives `[]`, `[]`, `{}` and `true` for those four. `direct-keys`,
        // `direct-json`, `hook-json` and `nested` are what THIS half fixes.
        // Do not put a `//` comment inside the literal below: the lines end
        // in `\` continuations, so it would become expected output.
        "sub-keys [\"__perryAsyncResourceBacking\"]\n\
         sub-gopn [\"__perryAsyncResourceBacking\",\"asyncId\",\"bind\",\"emitDestroy\",\"runInAsyncScope\",\"triggerAsyncId\"]\n\
         sub-json {\"__perryAsyncResourceBacking\":\"\"}\n\
         direct-keys []\n\
         direct-json {}\n\
         hook-json {}\n\
         nested {\"a\":{},\"b\":{}}\n\
         proto-chain false\n\
         instanceof true true true\n\
         typeof object object object\n\
         asyncId number number\n\
         trigger number\n\
         runInAsyncScope 2\n\
         bind function\n\
         hook-methods function function\n\
         identity true false\n\
         map 2 1 2\n"
    );
}

/// #10926 regression -- the resolver may not re-enter the property path.
///
/// `js_object_get_field_by_name` calls `try_async_resource_property_dispatch`
/// for EVERY receiver, and #10926 changed that entry point to RESOLVE the
/// receiver where it used to identity-check it. A resolver that reads an own
/// property therefore closes a cycle: `get_field_by_name` -> dispatch ->
/// `resolve_async_resource_handle` -> `get_field_by_name`. The key it reads
/// (`__perryAsyncResourceBacking`) is absent on ordinary objects, so the inner
/// lookup always misses and re-enters. Merely LINKING `node:async_hooks` was
/// then fatal: the first property miss in the program exhausted the 8 MB stack
/// and the binary died with SIGSEGV before printing anything. The first draft
/// of this split did exactly that.
#[test]
fn a_property_miss_does_not_recurse_once_async_hooks_is_linked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { AsyncResource } from "node:async_hooks";
const plain: any = { kept: 1 };
console.log("miss-plain", plain.absent === undefined, plain.__perryAsyncResourceBacking === undefined);
const res: any = new AsyncResource("PROBE");
console.log("miss-resource", res.absent === undefined, typeof res.asyncId());
console.log("linked", typeof AsyncResource, plain.kept);
"#,
    );
    assert_eq!(
        stdout,
        "miss-plain true true\n\
         miss-resource true number\n\
         linked function 1\n"
    );
}

/// The resources must survive a collection: the handle is an ordinary movable
/// object now, and its backing record is reached through `ObjectMeta`. A probe
/// that only called the methods immediately after construction would not cover
/// the axis the representation changes.
#[test]
fn async_resources_survive_a_collection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { AsyncResource, createHook } from "node:async_hooks";
class MyRes extends AsyncResource { constructor() { super("MYRES"); } }
const sub: any = new MyRes();
const direct: any = new AsyncResource("DIRECT");
const hook: any = createHook({ init() {} });
const idBefore = sub.asyncId();
let sink: any[] = [];
for (let i = 0; i < 200000; i++) { sink.push({ i: i, j: [i, i + 1] }); }
sink = [];
console.log("stable-id", sub.asyncId() === idBefore);
let ran = 0;
sub.runInAsyncScope(() => { ran++; });
direct.runInAsyncScope(() => { ran++; });
console.log("after-gc", ran, typeof direct.asyncId(), typeof hook.enable);
console.log("surface", JSON.stringify(sub), JSON.stringify(direct));
"#,
    );
    assert_eq!(
        stdout,
        // `sub` still carries the held own property, so its JSON is the
        // subclass half's business; `direct` is this half's.
        "stable-id true\n\
         after-gc 2 number function\n\
         surface {\"__perryAsyncResourceBacking\":\"\"} {}\n"
    );
}
