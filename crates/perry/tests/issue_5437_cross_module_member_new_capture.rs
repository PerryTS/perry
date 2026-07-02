//! Regression test for #5437 (Next.js dynamic/API routes — the `rJ`
//! constructor "value is not a function" blocker).
//!
//! Root: a function-nested class that captures an enclosing-scope local
//! (e.g. a hoisted sibling `function helper`) is exported from module A and
//! constructed via a member-callee `new ns.C(...)` in a DIFFERENT module B.
//! Because module B's codegen context doesn't contain class `C` (it lives in
//! module A), `try_static_class_name` can't resolve the member callee, so the
//! construct falls through to the runtime construct path
//! (`construct_registered_class_ref`) which supplies NO capture args. The
//! synthesized `__perry_cap_*` ctor params then bound to garbage/undefined and
//! the captured local (`helper`) read as a non-callable —
//! `new ns.C(); c.run()` threw `TypeError: value is not a function`.
//!
//! This is the cross-module variant of the W6 same-module member-new fix.
//! The earlier fix only filled captures from the decl-site snapshot when the
//! `new` site was statically routed to `lower_new_member_captured`; the
//! cross-module runtime construct path never reached it. The fix routes the
//! snapshot read INTO the synthesized constructor body itself
//! (`this.__perry_cap_X = js_class_capture_value_or(cid, slot, param)`), so
//! EVERY construction path — inline, member, or runtime cross-module —
//! recovers the captured value from the class's own home-module snapshot.
//!
//! Mirrors Next's app-route-turbo `rJ` ctor (`this.methods = r_(e)` where
//! `r_` is a hoisted sibling function captured by the class, constructed via
//! `new w.AppRouteRouteModule({...})` from the app-route template chunk).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn cross_module_member_new_recovers_captures_from_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Module A: a run-once factory IIFE that hoists a sibling `function
    // tag()` and declares a class `Mod` whose constructor and method both
    // call the captured `tag`. `Mod` is the default export, re-exported under
    // a namespace by B. This mirrors the giant turbopack module factory that
    // holds `function r_` + `class rJ { constructor(){ this.methods=r_(e) } }`.
    std::fs::write(
        dir.path().join("a.ts"),
        r#"
// A run-once factory IIFE (the turbopack module-factory shape) that hoists a
// sibling `function tag` captured by the class, and exposes the class under a
// DIFFERENT export name (`Mod`) than its internal class name (`InnerMod`) —
// exactly mirroring `class rJ {}` exported as `AppRouteRouteModule`. The
// export-name ≠ class-key mismatch is what makes the importing module unable
// to resolve the member callee statically, forcing the runtime construct
// path.
const built = (function () {
  function tag(x: any) {
    return "tag:" + String(x);
  }
  function plain(x: any) {
    return "plain:" + String(x);
  }
  class InnerMod {
    label: any;
    kind: any;
    constructor(opts: any) {
      // the throwing-site analog: `this.methods = r_(e)` — call the captured
      // sibling function inside the ctor.
      this.label = tag(opts.name);
      this.kind = plain(opts.name);
    }
    describe() {
      return tag(this.label);
    }
  }
  return { InnerMod };
})();
export const Mod: any = built.InnerMod;
"#,
    )
    .expect("write a.ts");

    // Module B: imports A as a NAMESPACE and constructs `new ns.Mod(...)` — a
    // CROSS-MODULE member-callee new of the captured class. `ns.Mod` cannot be
    // resolved to the class statically here (the class lives in module A, and
    // its key `InnerMod` ≠ the export `Mod`), so the construct routes to the
    // runtime construct path which supplies no capture args.
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import * as ns from "./a.ts";
const c: any = new (ns as any).Mod({ name: "hello" });
console.log(c.label);
console.log(c.kind);
console.log(c.describe());
"#,
    )
    .expect("write main.ts");

    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?}):\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status.code()
    );
    assert!(
        !stderr.contains("value is not a function"),
        "captured sibling function read as non-callable:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // tag("hello") = "tag:hello"; plain("hello") = "plain:hello";
    // describe() = tag("tag:hello") = "tag:tag:hello".
    let expected = "tag:hello\nplain:hello\ntag:tag:hello\n";
    assert_eq!(
        stdout, expected,
        "cross-module member-new must recover captured `tag` from the decl-site \
         snapshot (matches node):\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
