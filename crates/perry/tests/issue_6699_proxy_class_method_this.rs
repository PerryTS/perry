//! Regression tests for #6699 (pi TUI wall #13 / gate 3): a class method
//! invoked through a `Proxy` must run with `this === proxy`, so the method
//! body's `this.field` reads/writes route back through the proxy's traps —
//! exactly as node does.
//!
//! Trigger in the wild: pi's theme accessor is
//! `const theme = new Proxy({}, { get(_t, prop) { return
//! globalThis[THEME_KEY][prop]; } })` — an empty-target proxy whose get trap
//! forwards every read to the real `Theme` instance. The TUI render path calls
//! `theme.fg("...")`, and `Theme.fg` does `this.fgColors.get(color)`. Under
//! perry `this.fgColors` came back `undefined`, so `.get` threw
//! `TypeError: Cannot read properties of undefined (reading 'get')` where node
//! renders the full onboarding screen.
//!
//! Root cause: a canonical class-method VALUE (what the get trap returns for
//! `real.fg`) resolves its receiver through `canonical_bound_method_receiver`,
//! which reads the call-site `this` (IMPLICIT_THIS, correctly the proxy) but
//! then required it to be an above-handle-band heap object. A proxy id lives in
//! the handle band, so that check rejected it and the code fell through to
//! return the INT32 owner-marker — leaking `typeof this === "number"` (the same
//! marker-leak the #6475 closure case guards against). With `this` a bare
//! number, `this.fgColors` read off a non-proxy and missed the get trap.
//! Additionally, the typed-`this` class-field get/set fast path forwards the
//! full NaN-boxed (0x7FFD-tagged) receiver to `js_object_get/set_field_by_name`,
//! whose proxy-band test did not strip the tag, so `this.field` on a proxy
//! `this` had to be normalized there too.
//!
//! Expected outputs are node v26's, byte for byte.

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
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// pi's theme shape verbatim: an empty-target `Proxy` whose get trap forwards
/// to a `Theme` instance held under a global `Symbol`. Calling `theme.fg(name)`
/// must run `Theme.fg` with `this === proxy` so `this.fgColors.get(name)` reads
/// the real `Map` through the trap. Also pins the `this` identity/typeof that
/// the marker-leak corrupted.
#[test]
fn proxy_forwarded_class_method_reads_this_field_through_get_trap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Theme {
  fgColors;
  constructor(entries: [string, string][]) {
    this.fgColors = new Map(entries);
  }
  fg(name: string): string {
    const ansi = this.fgColors.get(name);
    if (!ansi) throw new Error("unknown color: " + name);
    return "<" + ansi + ">";
  }
  whoami(): unknown {
    return this;
  }
}
const KEY = Symbol.for("pi:theme");
(globalThis as any)[KEY] = new Theme([["red", "31"], ["green", "32"]]);
const theme: any = new Proxy({}, {
  get(_t, prop) {
    const t = (globalThis as any)[KEY];
    if (!t) throw new Error("Theme not initialized. Call initTheme() first.");
    return t[prop];
  },
});
console.log("fg red:", theme.fg("red"));
console.log("fg green:", theme.fg("green"));
console.log("this===proxy:", theme.whoami() === theme);
console.log("typeof this:", typeof theme.whoami());
"#,
    );
    assert_eq!(
        stdout,
        "fg red: <31>\n\
         fg green: <32>\n\
         this===proxy: true\n\
         typeof this: object\n"
    );
}

/// The symmetric write path: a proxy with both get and set traps forwarding to
/// the real instance. `Counter.bump` does `this.count = this.count + 1` with
/// `this === proxy`, so the field WRITE must route through the set trap (and
/// the read through the get trap) to mutate the real instance.
#[test]
fn proxy_forwarded_class_method_writes_this_field_through_set_trap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Counter {
  count;
  constructor() { this.count = 0; }
  bump(): number { this.count = this.count + 1; return this.count; }
  value(): number { return this.count; }
}
const KEY = Symbol.for("pi:counter");
(globalThis as any)[KEY] = new Counter();
const counter: any = new Proxy({}, {
  get(_t, prop) { return (globalThis as any)[KEY][prop]; },
  set(_t, prop, value) { (globalThis as any)[KEY][prop] = value; return true; },
});
console.log("bump:", counter.bump(), counter.bump(), counter.bump());
console.log("value:", counter.value());
"#,
    );
    assert_eq!(stdout, "bump: 1 2 3\nvalue: 3\n");
}
