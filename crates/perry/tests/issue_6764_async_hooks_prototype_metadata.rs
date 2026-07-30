//! Regression coverage for the first #6764 async_hooks parity increment:
//! constructor/prototype metadata and reflective prototype calls.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn async_hooks_constructors_expose_real_prototype_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";

function metadata(entries: Array<[string, unknown]>) {
  return entries
    .map(([name, value]) =>
      typeof value === "function"
        ? `${name}:${(value as Function).name}/${(value as Function).length}`
        : `${name}:missing`,
    )
    .join("|");
}

console.log(
  "storage:",
  metadata([
    ["constructor", AsyncLocalStorage],
    ["run", AsyncLocalStorage.prototype.run],
    ["getStore", AsyncLocalStorage.prototype.getStore],
    ["enterWith", AsyncLocalStorage.prototype.enterWith],
    ["exit", AsyncLocalStorage.prototype.exit],
    ["disable", AsyncLocalStorage.prototype.disable],
  ]),
);
console.log(
  "resource:",
  metadata([
    ["constructor", AsyncResource],
    ["asyncId", AsyncResource.prototype.asyncId],
    ["triggerAsyncId", AsyncResource.prototype.triggerAsyncId],
    ["emitDestroy", AsyncResource.prototype.emitDestroy],
    ["runInAsyncScope", AsyncResource.prototype.runInAsyncScope],
    ["bind", AsyncResource.prototype.bind],
  ]),
);

const storage = new AsyncLocalStorage<string>();
const storageResult = AsyncLocalStorage.prototype.run.call(
  storage,
  "ctx",
  (a: number, b: number) => `${storage.getStore()}:${a + b}`,
  2,
  3,
);
console.log("storage call:", storageResult);

const resource = new AsyncResource("fixture");
const resourceResult = AsyncResource.prototype.runInAsyncScope.call(
  resource,
  (a: number, b: number) => a + b,
  null,
  4,
  5,
);
console.log("resource call:", resourceResult);
console.log(
  "foreign no-op:",
  AsyncLocalStorage.prototype.enterWith.call({}, "value"),
  AsyncLocalStorage.prototype.disable.call({}),
);
"#,
    )
    .expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "storage: constructor:AsyncLocalStorage/0|run:run/2|getStore:getStore/0|",
            "enterWith:enterWith/1|exit:exit/1|disable:disable/0\n",
            "resource: constructor:AsyncResource/1|asyncId:asyncId/0|",
            "triggerAsyncId:triggerAsyncId/0|emitDestroy:emitDestroy/0|",
            "runInAsyncScope:runInAsyncScope/2|bind:bind/2\n",
            "storage call: ctx:5\n",
            "resource call: 9\n",
            "foreign no-op: undefined undefined\n",
        )
    );
}
