//! Design step 4, the loop region's STATIC supplier (DESIGN §4.1).
//!
//! A loop region whose receiver the compiler names (a class instance, `this`,
//! a closed object type) compares the receiver's ShapeId against the
//! driver's static id as an immediate and runs its fast copy with the slots
//! that id names, as constants: no word load, no prime. A receiver of any
//! other shape selects the generic copy (the plain loop).
//!
//! The route census (`PERRY_RECV_ROUTE_COUNT=1` at compile time) counts
//! `rloop_guard` (a guard ran), `rloop_static` (it hit the static id),
//! `rloop_plain` (the plain loop ran) and `rt_rloop_prime_ok` (the runtime
//! published a word). The output checks the slots the immediate word names:
//! a wrong slot reads the wrong field.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Three region loops over named receivers (a class parameter read, a class
/// parameter store, a closed object type), 50 entries each, then one class
/// instance that gained a key (its shape is not the birth shape): a static
/// miss, served by the plain loop.
const MAIN: &str = r#"
class P {
  a: number; b: number; c: number;
  constructor(a: number, b: number, c: number) { this.a = a; this.b = b; this.c = c; }
}
function readP(p: P, n: number): number { let s = 0; for (let i = 0; i < n; i++) { s += p.c * 2 + p.b; } return s; }
function bump(p: P, n: number): number { for (let i = 0; i < n; i++) { p.a = p.a + p.c; } return p.a; }
function lit(o: { u: number; v: number }, n: number): number { let s = 0; for (let i = 0; i < n; i++) { s += o.v - o.u; } return s; }
let t = 0;
for (let k = 0; k < 50; k++) {
  const p = new P(k, 2 * k + 1, 3 * k + 2);
  t += readP(p, 10) + bump(p, 5) + lit({ u: k, v: 5 * k + 3 }, 4);
}
const q: any = new P(7, 11, 13);
q.extra = 1;
console.log(t, readP(q, 10), bump(q, 2));
"#;

/// What node prints for [`MAIN`].
const EXPECTED: &str = "140800 370 33";

fn route(census: &str, name: &str) -> u64 {
    census
        .split_ascii_whitespace()
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no `{name}` in the route census:\n{census}"))
        .parse()
        .expect("a route count")
}

#[test]
fn named_receivers_run_their_loop_regions_from_the_static_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), MAIN).expect("write main.ts");
    let bin = dir.path().join("main");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join("main.ts"))
        .arg("-o")
        .arg(&bin)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_RECV_ROUTE_COUNT", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed:\n{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&bin).output().expect("run the program");
    assert!(run.status.success(), "the program failed: {run:?}");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let census = String::from_utf8_lossy(&run.stderr);
    assert_eq!(stdout.trim(), EXPECTED, "census:\n{census}");
    // Every entry of the three named loops is a static hit (150); `q`, whose
    // shape gained a key, misses into the plain loop (two entries). No region
    // loads or primes a word.
    assert_eq!(route(&census, "rloop_guard"), 152, "{census}");
    assert_eq!(route(&census, "rloop_static"), 150, "{census}");
    assert_eq!(route(&census, "rloop_split"), 150, "{census}");
    assert_eq!(route(&census, "rloop_plain"), 2, "{census}");
    assert_eq!(route(&census, "rt_rloop_prime_ok"), 0, "{census}");
}
