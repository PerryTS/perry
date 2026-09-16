//! Regression test for #10300: a class with no own constructor whose heritage
//! chain reaches one of the native bases perry stamps onto the INSTANCE
//! (`EventEmitter`, `Map`/`Set`, `Event`, ...) must get that surface however it
//! is constructed — including from a module other than the one that declares
//! it.
//!
//! The install rides on `super()`. A class with no own constructor writes no
//! `super()`, so the inline `new` lowering emits it at the call site
//! (#6325/#6326). A CROSS-MODULE `new` does not inline: it calls the defining
//! module's standalone `<class>_constructor` symbol, and that symbol did not
//! emit the install. `new C1()` from another module therefore produced a bare
//! object and `c.on(...)` threw `TypeError: on is not a function`.
//!
//! The shape is `@opentui/core`'s — `class KeyHandler extends EventEmitter {}`
//! and `class InternalKeyHandler extends KeyHandler { renderableHandlers = new
//! Map() }` declared in one chunk, `new InternalKeyHandler` plus
//! `.on("keypress", …)` performed from another. It is what stopped OpenCode's
//! TUI from painting.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// Declares the subclasses. Nothing here constructs the cross-module cases —
/// the defining module's own `new` already worked and is kept as a control.
const LIB: &str = "\
import { EventEmitter } from \"events\";
export class Bare extends EventEmitter {}
export class KeyHandler extends EventEmitter {}
export class InternalKeyHandler extends KeyHandler {
  renderableHandlers = new Map();
  pings = 0;
  // A field initializer that USES the inherited surface. `new Map()` alone
  // would still construct if the base init ran after the fields; calling
  // `this.on(...)` here cannot, so this pins the ORDER as well as the install.
  armed = (this.on(\"ping\", () => { this.pings++; }), true);
}
export class Seeded extends EventEmitter {
  hits = 0;
  constructor() {
    super();
    this.on(\"ping\", () => { this.hits++; });
  }
}
export class DerivedOfSeeded extends Seeded {}
export class SeededMap extends Map {}
export const madeInModule = new Bare();
";

/// Every `new` here crosses the module boundary.
const ENTRY: &str = "\
import {
  Bare, InternalKeyHandler, DerivedOfSeeded, SeededMap, madeInModule,
} from \"./lib.ts\";

const bare = new Bare();
let bareHits = 0;
bare.on(\"x\", () => { bareHits++; });
bare.emit(\"x\");
console.log(\"bare\", typeof bare.on, bareHits);

const k = new InternalKeyHandler();
let keyHits = 0;
k.on(\"keypress\", () => { keyHits++; });
k.emit(\"keypress\");
k.emit(\"ping\");
console.log(\"internal\", typeof k.on, keyHits, k.renderableHandlers.size, k.armed, k.pings);

const d = new DerivedOfSeeded();
d.emit(\"ping\");
console.log(\"derived\", typeof d.on, d.hits);

const m = new SeededMap([[\"k\", 9]]);
console.log(\"map\", m.size, m.get(\"k\"));

console.log(\"control\", typeof madeInModule.on);
";

/// Byte-for-byte what bun and node print for `ENTRY`.
const EXPECTED: &str = "\
bare function 1
internal function 1 0 true 1
derived function 1
map 1 9
control function
";

fn compile_and_run() -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("lib.ts"), LIB).unwrap();
    std::fs::write(root.join("entry.ts"), ENTRY).unwrap();

    let output = root.join("entry_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("entry.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "cross-module native-base subclasses must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

#[test]
fn cross_module_new_installs_the_native_base() {
    let stdout = compile_and_run();
    assert!(
        !stdout.contains("undefined"),
        "no cross-module instance may come back without its base surface; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a cross-module `new` must install the same native base the inline \
         `new` does, and must not re-run an ancestor constructor's init"
    );
}
