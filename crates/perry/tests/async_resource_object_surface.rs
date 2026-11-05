//! AsyncResource, AsyncHook and subclasses have Node's ordinary-object surface.
//! The subclass prototype chain supplies methods without leaking an own backing
//! field. Keep reflection, identity, dispatch and collection stability pinned
//! to Node 26.12.1; pristine main is checked when refreshing these expectations.

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

/// The complete ordinary-object surface matches the pinned Node oracle.
#[test]
fn async_resource_and_hook_match_nodes_object_surface() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(dir.path(), PROGRAM);
    assert_eq!(
        stdout,
        "sub-keys []\n\
         sub-gopn []\n\
         sub-json {}\n\
         direct-keys []\n\
         direct-json {}\n\
         hook-json {}\n\
         nested {\"a\":{},\"b\":{}}\n\
         proto-chain true\n\
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
        "stable-id true\n\
         after-gc 2 number function\n\
         surface {} {}\n"
    );
}
