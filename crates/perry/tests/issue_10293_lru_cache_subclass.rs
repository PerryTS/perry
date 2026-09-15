//! Regression test for #10293: `class X extends LRUCache` must construct.
//!
//! `lru-cache` is one of the packages perry serves from a bundled native
//! binding rather than compiling from npm source. The binding is a
//! COMPILE-TIME lowering with no runtime class value, so heritage that names
//! it reached the dynamic parent-registration path and threw `Class extends
//! value is not a constructor` before any user code ran.
//!
//! Recognising `LRUCache` as a native parent routes it to the same
//! subclass-init pattern `EventEmitter` and the `node:stream` bases use: the
//! runtime builds a real cache, stashes the handle on a hidden slot of `this`,
//! and installs the method surface directly onto the instance.
//!
//! The surface is deliberately PARTIAL — the six methods the binding
//! implements faithfully. `forEach`/`dispose`/`fetch`/the iterator protocol are
//! NOT installed, so a subclass that reaches for them throws at the call site
//! instead of inheriting the binding's documented silent no-ops. That is NOT
//! bun parity — bun runs the real package and `forEach` works there — it is a
//! deliberate choice to fail loudly rather than answer wrongly, and the second
//! test pins it so it stays a decision rather than an accident.
//!
//! Both `has` and `delete` normalise their result to a real boolean. Two
//! `js_lru_cache_has`/`js_lru_cache_delete` symbols exist — perry-stdlib's
//! legacy pair answers a NUMBER and perry-ext-lru-cache's answers a NaN-boxed
//! boolean — and the DIRECT `cache.has(k)` lowering reaches the ext one, so
//! without normalising, a subclass disagreed with its own base in one program:
//! `base.has(k)` was `true` while `sub.has(k)` was `1`.

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

const SUBCLASS: &str = r#"
import { LRUCache } from "lru-cache"

class Store extends LRUCache<string, number> {
  hits = 0
  bump(k: string) {
    this.hits++
    return this.get(k)
  }
}

const s = new Store({ max: 3 })
s.set("a", 1)
s.set("b", 2)
console.log("get:", s.get("a"), s.get("b"))
console.log("has:", s.has("a"), s.has("zzz"))
console.log("peek:", s.peek("b"))
console.log("own field:", s.bump("a"), s.hits)
s.delete("a")
console.log("after delete:", s.has("a"), s.get("b"))
s.clear()
console.log("after clear:", s.has("b"))
"#;

const EXPECTED: &str = "\
get: 1 2
has: true false
peek: 2
own field: 1 1
after delete: false 2
after clear: false
";

/// The surface perry does NOT install must throw, not silently answer.
const PARTIAL_SURFACE: &str = r#"
import { LRUCache } from "lru-cache"
class Store extends LRUCache<string, number> {}
const s: any = new Store({ max: 2 })
s.set("a", 1)
try {
  s.forEach(() => {})
  console.log("forEach: did not throw")
} catch (e: any) {
  console.log("forEach: threw")
}
"#;

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), source).unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "`class X extends LRUCache` must compile; stdout:\n{}\nstderr:\n{}",
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
fn lru_cache_subclass_constructs_and_caches() {
    let stdout = compile_and_run(SUBCLASS);
    assert!(
        !stdout.contains("not a constructor"),
        "heritage naming a compile-time binding must not reach dynamic parent \
         registration; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a subclass instance must carry the cache surface AND its own fields"
    );
}

/// NOT a bun-parity assertion: bun runs the real package, where `forEach`
/// works. This pins perry's deliberate boundary — the binding implements six
/// methods faithfully and a subclass reaching past them must throw at the call
/// site rather than inherit a silent no-op.
#[test]
fn uninstalled_surface_throws_rather_than_lying() {
    assert_eq!(
        compile_and_run(PARTIAL_SURFACE),
        "forEach: threw\n",
        "only the faithfully-implemented methods are installed; the rest must \
         fail loudly instead of inheriting the binding's silent no-ops"
    );
}
