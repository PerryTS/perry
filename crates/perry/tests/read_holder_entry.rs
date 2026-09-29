//! The read site's holder entry (`object::method_site::read_holder`): a key
//! that is not own on the receiver is answered from facts of the receiver's
//! and the holder's shapes, compared on every use.
//!
//! One program, run at two trip counts taken from argv (a fixed small loop is
//! unrolled and a literal receiver refined, so neither would reach the site),
//! changes the chain MID-loop in every way the entry must see: a shadowing own
//! key, a value store to the holder, a key added to and deleted from the
//! holder, a getter defined on it, `setPrototypeOf` on the receiver and on an
//! intermediate hop, a key added to a hop at depth 2 and 3, `F.prototype`
//! replaced per iteration, a key added to `Object.prototype` under an absent
//! entry, a collection that moves a holder that was young at prime time, the
//! exotic receivers (`process.env`, `arguments`) and a class accessor. Each
//! line prints the value read and the value node prints; the entry must have
//! primed (data and absent), or the program proves nothing about it.

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = r#"// Runtime trip counts (argv), every chain change MID-loop, parameter
// receivers: a fixed small loop would be unrolled and the receiver refined.
const N = Number(process.argv[2] ?? "40");
const H = N >> 1;
const lines: string[] = [];
function check(name: string, got: number, want: number): void {
  lines.push(name + " " + got + " " + want + (got === want ? " ok" : " FAIL"));
}
function num(v: any, missing: number): number {
  return v === undefined ? missing : v;
}

// 1. shadow: an own key added to the receiver
function t1(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { if (i === H) o.k = 100; s += o.k; }
  return s;
}
check("shadow", t1(Object.create({ k: 1 }), N), H + (N - H) * 100);

// 2. a value store to the holder's slot
function t2(o: any, p: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { if (i === H) p.k = 5; s += o.k; }
  return s;
}
const P2: any = { k: 1 };
check("holder-value", t2(Object.create(P2), P2, N), H + (N - H) * 5);

// 3. another key added to the holder, then a value store
function t3(o: any, p: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i === H) p.other = 9;
    if (i === H + 1) p.k = 7;
    s += o.k;
  }
  return s;
}
const P3: any = { k: 1 };
check("holder-restamp", t3(Object.create(P3), P3, N), H + 1 + (N - H - 1) * 7);

// 4. delete from the holder; a getter defined on the holder
function t4(o: any, p: any, n: number, mode: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i === H) {
      if (mode === 0) delete p.k;
      else Object.defineProperty(p, "k", { get() { return 50; }, configurable: true });
    }
    s += num(o.k, 1000);
  }
  return s;
}
const P4a: any = { k: 1, j: 2 };
check("holder-delete", t4(Object.create(P4a), P4a, N, 0), H + (N - H) * 1000);
const P4b: any = { k: 1 };
check("holder-getter", t4(Object.create(P4b), P4b, N, 1), H + (N - H) * 50);

// 5. setPrototypeOf on the receiver; on the intermediate hop (depth 2)
function t5(o: any, target: any, to: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { if (i === H) Object.setPrototypeOf(target, to); s += num(o.k, 1000); }
  return s;
}
const o5a = Object.create({ k: 1 });
check("recv-setproto", t5(o5a, o5a, { k: 3 }, N), H + (N - H) * 3);
const mid5 = Object.create({ k: 1 });
check("hop-setproto", t5(Object.create(mid5), mid5, { k: 4 }, N), H + (N - H) * 4);

// 6. depth 3: the key added to an intermediate hop shadows the holder's
function t6(o: any, hop: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { if (i === H) hop.k = 6; s += o.k; }
  return s;
}
const C6: any = { k: 1 };
const B6 = Object.create(C6);
const A6 = Object.create(B6);
check("depth3-hop", t6(Object.create(A6), B6, N), H + (N - H) * 6);
const C6b: any = { k: 1 };
const A6b = Object.create(C6b);
check("depth2-hop", t6(Object.create(A6b), A6b, N), H + (N - H) * 6);

// 7. F.prototype replaced every iteration
function F7(this: any) {}
function read7(o: any): number { return o.k; }
function t7(n: number): number {
  const PA = { k: 1 }, PB = { k: 2 };
  let s = 0;
  for (let i = 0; i < n; i++) {
    (F7 as any).prototype = (i & 1) ? PB : PA;
    s += read7(new (F7 as any)());
  }
  return s;
}
check("ctor-prototype", t7(N), (N - (N >> 1)) * 1 + (N >> 1) * 2);

// 8. absent: a key added to Object.prototype mid-loop (depth 1: a literal;
// depth 2: an Object.create child)
function t8(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i === H) (Object.prototype as any).zz8 = 8;
    s += num(o.zz8, 0);
  }
  delete (Object.prototype as any).zz8;
  return s;
}
check("absent-d1", t8({ a: 1 }, N), (N - H) * 8);
check("absent-d2", t8(Object.create({ a: 1 }), N), (N - H) * 8);
function t8b(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += num(o.zz, 1);
  return s;
}
check("absent-steady", t8b({ a: 1, b: 2 }, N), N);

// 9. GC moves the holder: it is young at prime time
function t9(n: number): number {
  const p: any = { k: 1 };
  const o = Object.create(p);
  let s = 0;
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    if (i === H) {
      for (let j = 0; j < 20000; j++) keep.push({ j });
      (globalThis as any).gc();
      keep = [];
    }
    if (i === H + 1) p.k = 11;
    s += o.k;
  }
  return s;
}
check("gc-moves-holder", t9(N), H + 1 + (N - H - 1) * 11);

// Exotic receivers: process.env, arguments
function t10(e: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i === H) e.PERRY_HOLDER_TEST_KEY = "v";
    s += e.PERRY_HOLDER_TEST_KEY === undefined ? 0 : 1;
  }
  return s;
}
check("exotic-env", t10(process.env, N), N - H);
function t11(n: number, ...rest: any[]): number {
  let s = 0;
  const a: any = (function (this: any) { return arguments; })(1, 2);
  for (let i = 0; i < n; i++) {
    if (i === H) (Object.prototype as any).zz11 = 3;
    s += num(a.zz11, 0);
  }
  delete (Object.prototype as any).zz11;
  return s;
}
check("exotic-arguments", t11(N), (N - H) * 3);

// Class accessor: never answered as a slot
class C12 { n = 1; get k(): number { return this.n * 2; } }
function t12(o: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { if (i === H) o.n = 5; s += o.k; }
  return s;
}
check("class-accessor", t12(new C12(), N), H * 2 + (N - H) * 10);

console.log(lines.join("\n"));
"#;

fn run(n: &str) -> (String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .arg(n)
        .current_dir(dir.path())
        .env("PERRY_METHOD_SITE_STATS", "1")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_POISON_FROMSPACE", "1")
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{stderr}",
        run.status
    );
    (String::from_utf8_lossy(&run.stdout).into_owned(), stderr)
}

fn stat(stderr: &str, name: &str) -> u64 {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("[method-site]"))
        .unwrap_or_else(|| panic!("no [method-site] line in:\n{stderr}"));
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no {name} in {line}"))
}

#[test]
fn holder_entry_follows_every_chain_change() {
    for n in ["40", "41"] {
        let (stdout, stderr) = run(n);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 17, "n={n}: expected 17 checks:\n{stdout}");
        let failed: Vec<&&str> = lines.iter().filter(|l| !l.ends_with(" ok")).collect();
        assert!(failed.is_empty(), "n={n}: wrong reads {failed:?}\n{stdout}");
        assert!(
            stat(&stderr, "read_holder_primes") > 0,
            "n={n}: no data entry primed\n{stderr}"
        );
        assert!(
            stat(&stderr, "read_absent_primes") > 0,
            "n={n}: no absent entry primed\n{stderr}"
        );
    }
}
