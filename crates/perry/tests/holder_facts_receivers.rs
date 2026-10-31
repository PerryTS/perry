//! Holder facts: a receiver that is itself a field read (`this.current.run()`,
//! `this.current.id`) is proven when the field's facts name the classes of
//! every object any write stores there (`FactsField::classes`), over every
//! class of the field's holder. The read lowers through the facts (a slot
//! load, the class-id switch of direct calls), with no probe.
//!
//! No fact when the field can hold anything else: ⊤ (a thrown value), a
//! function, or when interpreted code can run. A nullable field keeps the
//! receiver test (null throws). A key the constructor assigns after code
//! that observes `this` can be read unset (reserved): no field-class fact. Per unit never
//! proves more than the whole program.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

struct Built {
    /// Saved LLVM IR of module `<name>_ts.ll`.
    ll: PathBuf,
    /// The `<dump>.elisions` record: one line per elided field access.
    elisions: String,
    stdout: String,
}

impl Built {
    fn ir(&self, module: &str) -> String {
        std::fs::read_to_string(self.ll.join(format!("{module}_ts.ll"))).expect("saved ll")
    }

    /// Elided accesses in `Sched.<method>` whose receiver is the field read
    /// `this.<field>`.
    fn field_elisions(&self, method: &str, field: &str) -> usize {
        self.elisions
            .lines()
            .filter(|l| {
                l.contains(&format!("__Sched__{method}"))
                    && l.contains(&format!("recv=field {field} of "))
            })
            .count()
    }

    fn field_receiver_elisions(&self, method: &str) -> usize {
        self.field_elisions(method, "current")
    }
}

/// Write `files` (the first is the entry), compile (whole program when
/// `whole`, else per unit), run.
fn build(dir: &Path, files: &[(&str, &str)], whole: bool) -> Built {
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let tag = if whole { "w" } else { "u" };
    let exe = dir.join(format!("main_{tag}"));
    let ll = dir.join(format!("ll_{tag}"));
    std::fs::create_dir_all(&ll).unwrap();
    let dump = dir.join(format!("facts_{tag}"));
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll)
        .env("PERRY_FACTS_DUMP", &dump);
    if whole {
        cmd.env("PERRY_FACTS_UNITS", "0");
    }
    let out = cmd
        .arg("compile")
        .arg(dir.join(files[0].0))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed (whole={whole})\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed (whole={whole}): {:?}", ran.status);
    let mut el = dump.into_os_string();
    el.push(".elisions");
    Built {
        ll,
        elisions: std::fs::read_to_string(el).unwrap_or_default(),
        stdout: String::from_utf8_lossy(&ran.stdout).into_owned(),
    }
}

/// The body of the function defined as `@<symbol>(`.
fn body<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let head = format!("@{symbol}(");
    let at = ir
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| {
            ir[i..]
                .lines()
                .next()
                .is_some_and(|line| line.contains(&head))
        })
        .unwrap_or_else(|| panic!("no function {symbol}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

/// Both modes: the program prints `expected`; return (per unit, whole).
fn both(files: &[(&str, &str)], expected: &str) -> (Built, Built, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let u = build(dir.path(), files, false);
    let w = build(dir.path(), files, true);
    assert_eq!(u.stdout.trim(), expected, "per unit output");
    assert_eq!(w.stdout.trim(), expected, "whole output");
    (u, w, dir)
}

const PROBES: [&str; 3] = ["@perry_probe_get(", "@perry_probe_family_method(", "@js_method_call_slow("];

fn assert_no_probe(ir: &str, symbol: &str) {
    let b = body(ir, symbol);
    for p in PROBES {
        assert!(!b.contains(p), "{symbol} keeps {p}:\n{b}");
    }
}

fn assert_probe(ir: &str, symbol: &str) {
    let b = body(ir, symbol);
    assert!(PROBES.iter().any(|p| b.contains(p)), "{symbol} has no generic site:\n{b}");
}

const SINGLE: &str = r#"
class Task {
  id: number; link: Task | null;
  constructor(id: number) { this.id = id; this.link = null; }
  run(n: number): number { return this.id * 2 + n; }
}
class Sched {
  current: Task;
  constructor() { this.current = new Task(3); }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) acc += s.step(i);
console.log(acc);
"#;
const UNION: &str = r#"
class Worker {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Device {
  id: number; pad: number;
  constructor(id: number) { this.pad = 7; this.id = id; }
  run(n: number): number { return this.id * n; }
}
class Sched {
  current: Worker | Device;
  constructor(d: boolean) { this.current = d ? new Device(2) : new Worker(5); }
  swap(): void { this.current = this.current instanceof Worker ? new Device(3) : new Worker(4); }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched(false);
let acc = 0;
for (let i = 0; i < 1000; i++) { acc += s.step(i); if (i % 3 === 0) s.swap(); }
console.log(acc);
"#;
const NULLABLE: &str = r#"
class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Sched {
  current: Task | null;
  constructor() { this.current = null; }
  set(t: Task | null): void { this.current = t; }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) { s.set(new Task(i)); acc += s.step(i); }
s.set(null);
try { s.step(1); } catch (e) { console.log(e instanceof TypeError); }
console.log(acc);
"#;
const TOP: &str = r#"
class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Sched {
  current: any;
  ok: Task;
  constructor() { this.current = new Task(1); this.ok = new Task(2); }
  poison(): void { try { (null as any).x; } catch (e) { this.current = e; } }
  stepOk(n: number): number { return this.ok.run(n) + this.ok.id; }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) acc += s.step(i) + s.stepOk(i);
s.poison();
try { s.step(1); } catch (e) { console.log(e instanceof TypeError); }
console.log(acc);
"#;
const FUNC: &str = r#"
class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Box {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Sched {
  current: any;
  ok: Box;
  constructor() { this.current = new Task(1); this.ok = new Box(2); }
  fn(): void { const f: any = (n: number) => n; f.id = 9; f.run = (n: number) => n * 3; this.current = f; }
  stepOk(n: number): number { return this.ok.run(n) + this.ok.id; }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) acc += s.step(i) + s.stepOk(i);
s.fn();
console.log(acc + s.step(2));
"#;
const INTERP: &str = r#"
class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Sched {
  current: Task;
  constructor() { this.current = new Task(3); }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) acc += s.step(i);
console.log(acc + (0, eval)("1 + 1"));
"#;
const EARLY: &str = r#"
class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
class Sched {
  current: Task;
  constructor(early: boolean) { if (early) this.peek(); this.current = new Task(3); }
  peek(): number { return this.current.run(1) + this.current.id; }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
const s = new Sched(false);
let acc = 0;
for (let i = 0; i < 1000; i++) acc += s.step(i);
console.log(acc);
try { new Sched(true); } catch (e) { console.log(e instanceof TypeError); }
"#;
const XMOD_HOLDER: &str = r#"
export class Task {
  id: number;
  constructor(id: number) { this.id = id; }
  run(n: number): number { return this.id + n; }
}
export class Sched {
  current: any;
  ok: Task;
  constructor() { this.current = new Task(3); this.ok = new Task(2); }
  stepOk(n: number): number { return this.ok.run(n) + this.ok.id; }
  step(n: number): number { return this.current.run(n) + this.current.id; }
}
"#;
const XMOD_OTHER: &str = r#"
import { Sched } from "./holder";
export class Other {
  pad: number; id: number;
  constructor() { this.pad = 1; this.id = 100; }
  run(n: number): number { return n * 10; }
}
export function swap(s: Sched): void { s.current = new Other(); }
"#;
const XMOD_MAIN: &str = r#"
import { Sched } from "./holder";
import { swap } from "./other";
const s = new Sched();
let acc = 0;
for (let i = 0; i < 1000; i++) { acc += s.step(i) + s.stepOk(i); if (i === 500) swap(s); }
console.log(acc);
"#;

#[test]
fn a_field_of_one_class_is_a_proven_receiver() {
    let (u, w, _d) = both(&[("main.ts", SINGLE)], "508500");
    for b in [&u, &w] {
        assert!(b.field_receiver_elisions("step") > 0, "{}", b.elisions);
        let ir = b.ir("main");
        assert_no_probe(&ir, "perry_method_main_ts__Sched__step");
        assert!(body(&ir, "perry_method_main_ts__Sched__step")
            .contains("call double @perry_method_main_ts__Task__run("));
    }
}

#[test]
fn a_field_of_a_union_of_classes_switches_over_them() {
    let (u, w, _d) = both(&[("main.ts", UNION)], "1005997");
    for b in [&u, &w] {
        assert!(b.field_receiver_elisions("step") > 0, "{}", b.elisions);
        let ir = b.ir("main");
        let step = body(&ir, "perry_method_main_ts__Sched__step");
        assert_no_probe(&ir, "perry_method_main_ts__Sched__step");
        assert!(step.contains("call double @perry_method_main_ts__Worker__run("), "{step}");
        assert!(step.contains("call double @perry_method_main_ts__Device__run("), "{step}");
    }
}

#[test]
fn a_nullable_field_keeps_the_receiver_test() {
    let (u, w, _d) = both(&[("main.ts", NULLABLE)], "true\n1498500");
    for b in [&u, &w] {
        assert!(
            b.elisions.lines().any(|l| l.contains("__Sched__step")
                && l.contains("recv=field current ")
                && l.contains("nullable=1")),
            "{}",
            b.elisions
        );
        assert!(
            !b.elisions.lines().any(|l| l.contains("recv=field current ") && l.contains("nullable=0")),
            "{}",
            b.elisions
        );
    }
}

#[test]
fn a_field_written_top_has_no_fact() {
    let (u, w, _d) = both(&[("main.ts", TOP)], "true\n1005000");
    for b in [&u, &w] {
        // The control: a field only one class is ever stored into.
        assert!(b.field_elisions("stepOk", "ok") > 0, "{}", b.elisions);
        assert_eq!(b.field_receiver_elisions("step"), 0, "{}", b.elisions);
        assert_probe(&b.ir("main"), "perry_method_main_ts__Sched__step");
    }
}

#[test]
fn a_function_in_the_field_has_no_fact() {
    let (u, w, _d) = both(&[("main.ts", FUNC)], "1005015");
    for b in [&u, &w] {
        assert!(b.field_elisions("stepOk", "ok") > 0, "{}", b.elisions);
        assert_eq!(b.field_receiver_elisions("step"), 0, "{}", b.elisions);
        assert_probe(&b.ir("main"), "perry_method_main_ts__Sched__step");
    }
}

/// A negative guard only: with interpreted code no receiver fact exists at
/// all, so there is no control to prove (the interp sabotage makes it fail).
#[test]
fn interpreted_code_leaves_no_fact() {
    let (u, w, _d) = both(&[("main.ts", INTERP)], "505502");
    for b in [&u, &w] {
        assert_eq!(b.field_receiver_elisions("step"), 0, "{}", b.elisions);
    }
}

#[test]
fn a_reserved_key_gives_no_field_class_fact() {
    // `current` is stored after `this.peek()` ran: it can be read unset
    // (reserved: known slot, presence unknown), so no read of it is a proven
    // receiver, in any method.
    let (u, w, _d) = both(&[("main.ts", EARLY)], "505500\ntrue");
    for b in [&u, &w] {
        assert!(
            !b.elisions.lines().any(|l| l.contains("recv=field current ")),
            "a reserved key is a proven receiver:\n{}",
            b.elisions
        );
    }
}

#[test]
fn another_modules_writer_is_in_the_field_classes() {
    let files = [("main.ts", XMOD_MAIN), ("holder.ts", XMOD_HOLDER), ("other.ts", XMOD_OTHER)];
    let (u, w, _d) = both(&files, "4424156");
    for b in [&u, &w] {
        assert!(b.field_elisions("stepOk", "ok") > 0, "{}", b.elisions);
    }
    assert!(
        u.field_receiver_elisions("step") <= w.field_receiver_elisions("step"),
        "per unit proves more than whole:\n{}\n--\n{}",
        u.elisions,
        w.elisions
    );
    for b in [&u, &w] {
        assert!(
            !b.elisions.lines().any(|l| l.contains("recv=field current ") && !l.contains("Other")),
            "a field-read receiver proof without the other module's class:\n{}",
            b.elisions
        );
    }
}
