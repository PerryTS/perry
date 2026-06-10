//! Regression test for #4872: undefined default-wrapper symbols for
//! re-exported barrel modules (the nestjs link wall).
//!
//! Three shapes, all taken from the `tests/release/packages/nestjs-hello`
//! fixture's failure:
//!
//! 1. A tsc-emitted TYPE-ONLY module whose entire body is
//!    `Object.defineProperty(exports, "__esModule", { value: true });`
//!    (nestjs dist `*.interface.js`). Pre-fix it wasn't detected as CJS, so
//!    it compiled as a zero-export ES module; the consumer's synthetic
//!    `require()` still value-read its default import and the link died on
//!    `__perry_wrap_perry_fn_<src>__default` (and, once that was fixed, the
//!    unwrapped module threw `ReferenceError: exports is not defined` at
//!    init).
//!
//! 2. `__exportStar(require("./x"), exports)` barrels (tsc's CJS lowering of
//!    `export * from "./x"`), nested two levels deep
//!    (`@nestjs/common/index.js` → `decorators/index.js` → leaf). Pre-fix
//!    the wrap surfaced no static re-export, so the consumer's named import
//!    bound `perry_fn_<barrel>__Controller` — which no object file defines.
//!
//! 3. A default import (synthesized by the CJS wrap from `require(...)`) of
//!    an ES module that has ONLY named exports (rxjs `src/index.ts`, uid
//!    `dist/index.mjs`). There is no `default` binding to call, so the local
//!    now binds the module NAMESPACE (Node `require(esm)` semantics) and
//!    member calls resolve per-export to origin symbols.
//!
//! Fixes (see PR for #4872): cjs_wrap emits `export * from '<spec>'` for
//! `__exportStar` calls; `is_commonjs` detects `defineProperty(exports,`;
//! compile.rs routes default imports of no-default modules through the
//! namespace machinery; `export *` propagation no longer leaks `default`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn barrel_default_imports_and_export_star_chains_link_and_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "barrel-default-reexports",
  "type": "module",
  "perry": {
    "compilePackages": ["fakepkg"],
    "allow": { "compilePackages": ["fakepkg"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("fakepkg");
    std::fs::create_dir_all(&pkg).expect("mkdir fakepkg");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "fakepkg", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write fakepkg package.json");

    // Shape 1: type-only interface surface — the nestjs `*.interface.js`
    // dist output. No exports, no require, just the interop marker.
    std::fs::write(
        pkg.join("iface.js"),
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\n",
    )
    .expect("write iface.js");

    // Shape 3: ES module with ONLY named exports — no default binding.
    std::fs::write(
        pkg.join("esm-barrel.mjs"),
        "export const uid = (n) => \"U:\" + n;\n",
    )
    .expect("write esm-barrel.mjs");

    // Shape 2: two-level `__exportStar` chain down to a concrete leaf.
    std::fs::write(
        pkg.join("leaf.js"),
        r#""use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Controller = void 0;
function Controller() { return "CTRL"; }
exports.Controller = Controller;
"#,
    )
    .expect("write leaf.js");
    std::fs::write(
        pkg.join("mid.js"),
        r#""use strict";
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) exports[p] = m[p];
};
Object.defineProperty(exports, "__esModule", { value: true });
__exportStar(require("./leaf"), exports);
"#,
    )
    .expect("write mid.js");

    // The package barrel: star re-exports the type-only surface AND the
    // mid-level barrel, plus a member call on the no-default ES module.
    std::fs::write(
        pkg.join("index.js"),
        r#""use strict";
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) exports[p] = m[p];
};
Object.defineProperty(exports, "__esModule", { value: true });
__exportStar(require("./iface"), exports);
__exportStar(require("./mid"), exports);
const esm_1 = require("./esm-barrel.mjs");
exports.greet = function greet() { return esm_1.uid(7); };
"#,
    )
    .expect("write index.js");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { greet, Controller } from "fakepkg";
// Shape 3: member call on a default-import of a no-default ES module.
console.log(greet());
// Shape 2: named import resolved through the two-level __exportStar chain.
console.log(Controller());
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (link wall regressed?)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "U:7\nCTRL\n",
        "barrel re-exports must resolve to concrete origin bindings"
    );
}
