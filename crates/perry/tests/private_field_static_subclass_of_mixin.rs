//! A class DECLARATION extending a class produced by a mixin factory
//! (`class Sub extends mixin(Base) {}`) could not initialize the mixin's
//! private fields: `new Sub()` threw "Cannot write private member #x to an
//! object whose class did not declare it" from the mixin's own constructor.
//!
//! Every call of the factory is a fresh ClassDefinitionEvaluation with its own
//! private brand. `new <class value>` stamps that brand on the instance before
//! the constructor runs, but `new Sub()` allocates through the static class
//! and reaches the mixin's constructor through `super()`, which replayed it on
//! an unbranded receiver. `@npmcli/arborist` builds `Arborist` exactly this
//! way (`mixins.reduce((a, b) => b(a), EventEmitter)`, then
//! `class Arborist extends Base`), so OpenCode's dependency install died
//! with that TypeError.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, file_name: &str, source: &str) -> String {
    let entry = dir.join(file_name);
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The factory shapes around the failing one; every line is what Node 26.8.1
/// prints for the same source.
const SOURCE: &str = r#"
// @ts-nocheck
import { EventEmitter } from "node:events"
function t(label, fn) { try { console.log(label, fn()) } catch (e) { console.log(label, "THROW", e.message) } }
const mk = (cls) => class A extends cls {
  #x
  #m() { return "m" + this.#x }
  constructor(...a) { super(...a); this.#x = 1 }
  get x() { return this.#x }
  callM() { return this.#m() }
  static hasX(o) { return #x in o }
}
class Plain {}
t("factory", () => new (mk(Plain))().x)
t("factory twice", () => new (mk(mk(Plain)))().x)
class Sub extends mk(Plain) { constructor() { super(); this.y = 2 } }
t("static subclass", () => { const o = new Sub(); return [o.x, o.y, o.callM()].join(",") })
class Implicit extends mk(EventEmitter) {}
t("implicit ctor", () => new Implicit().x)
class Twice extends mk(mk(Plain)) {}
t("over two evaluations", () => new Twice().x)
class Grand extends Sub {}
t("grandchild", () => new Grand().x)
const M = mk(Plain)
class Branded extends M {}
t("brand check", () => [M.hasX(new Branded()), M.hasX(new Sub()), M.hasX({})].join(","))
"#;

const EXPECTED: &str = "factory 1\n\
factory twice 1\n\
static subclass 1,2,m1\n\
implicit ctor 1\n\
over two evaluations 1\n\
grandchild 1\n\
brand check true,false,false\n";

#[test]
fn static_subclass_of_a_mixin_initializes_its_private_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(compile_and_run(dir.path(), "main.ts", SOURCE), EXPECTED);
}

/// `@npmcli/arborist`'s construction: each mixin in its own CommonJS file,
/// folded over EventEmitter, then a class declaration on top, built again
/// through `this.constructor`.
const TRACKER_SOURCE: &str = r#"
module.exports = (cls) => class Tracker extends cls {
  #progress
  constructor(options) { super(options); this.#progress = new Map() }
  get tracked() { return this.#progress.size }
}
"#;

const BUILDER_SOURCE: &str = r#"
module.exports = (cls) => class IdealTreeBuilder extends cls {
  #strictPeerDeps
  constructor(options) { super(options); this.#strictPeerDeps = !!options.strictPeerDeps }
  get strict() { return this.#strictPeerDeps }
}
"#;

const ARBORIST_SOURCE: &str = r#"
const mixins = [require("./tracker.js"), require("./builder.js")]
const Base = mixins.reduce((a, b) => b(a), require("node:events"))
class Arborist extends Base {
  constructor(options) { super(options); this.path = options.path }
}
module.exports = Arborist
"#;

const ARBORIST_MAIN: &str = r#"
// @ts-nocheck
import Arborist from "./arborist.js"
const a = new Arborist({ path: "/p", strictPeerDeps: true })
console.log(a.path, a.strict, a.tracked, typeof a.on)
const b = new a.constructor({ path: "/q" })
console.log(b.path, b.strict, b.tracked)
"#;

const ARBORIST_EXPECTED: &str = "/p true 0 function\n/q false 0\n";

#[test]
fn arborist_mixin_chain_initializes_every_private_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("tracker.js"), TRACKER_SOURCE).unwrap();
    std::fs::write(dir.path().join("builder.js"), BUILDER_SOURCE).unwrap();
    std::fs::write(dir.path().join("arborist.js"), ARBORIST_SOURCE).unwrap();
    assert_eq!(
        compile_and_run(dir.path(), "main.ts", ARBORIST_MAIN),
        ARBORIST_EXPECTED
    );
}
