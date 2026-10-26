//! Per-unit mode: a closed class's constructor fed its own instances
//! (DECISIONS 109-111). js-compiled 23-binary-trees: `new Node(l, r)` passes
//! Node instances to Node's own constructor, and `check` reads `n.left` /
//! `n.right` of instances built in another unit.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

const TREE: &str = r#"
    class Node {
      left: Node | null;
      right: Node | null;
      constructor(left: Node | null, right: Node | null) { this.left = left; this.right = right; }
    }
    function bottomUp(depth: number): Node {
      if (depth > 0) return new Node(bottomUp(depth - 1), bottomUp(depth - 1));
      return new Node(null, null);
    }
    function check(n: Node): number {
      const l: Node | null = n.left;
      const r: Node | null = n.right;
      if (l === null || r === null) return 1;
      return 1 + check(l) + check(r);
    }
    console.log(check(bottomUp(4)));
    "#;

/// The merged head (main's call and marker rules over 03a's contracts): per
/// unit proves no site the whole program does not, and breaks `broken`.
/// DECISIONS 190 (callbacks and method calls through a described
/// parameter now reach an unknown callee) and 191 (a composed callee's
/// parameter has no MRK family, so a result holding it is another call's)
/// restore what the whole program proves.
fn assert_merged(pf: &ProgramFacts, wf: &ProgramFacts, broken: &[&str]) {
    let proven = |f: &ProgramFacts| -> std::collections::BTreeSet<String> {
        f.sites
            .iter()
            .filter(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
            .map(|s| format!("{}@{}.{}", s.module, s.byte_offset, s.key))
            .collect()
    };
    let (pu, pw) = (proven(pf), proven(wf));
    let stronger: Vec<&String> = pu.difference(&pw).collect();
    assert!(
        stronger.is_empty(),
        "per unit claims more: {stronger:?}\n{}",
        pf.dump_text()
    );
    assert_eq!(pf.broken, broken, "{}", pf.dump_text());
}

#[test]
fn a_constructor_fed_its_own_instances_keeps_its_class() {
    // `bottomUp`'s unit hands its Node stand-ins to Node's constructor
    // (DECISIONS 109): the class is not broken, and its fields hold what
    // the whole-program solve finds. `check`'s unit reads `n.left` as
    // Node's stand-in (DECISIONS 110), and a load from `n`'s markers adds
    // no builtin member (DECISIONS 111): the reads are T0, as whole.
    let pf = units(TREE);
    let whole = facts(TREE);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["left", "right"] {
        let (u, w) = (field(tmpl(&pf, "Node"), k), field(tmpl(&whole, "Node"), k));
        assert!(u.readonly && !u.kind.top, "{k}\n{}", pf.dump_text());
        assert_eq!(u.kind.objs, w.kind.objs, "{k}\n{}", pf.dump_text());
        assert_eq!(u.kind.prims, w.kind.prims, "{k}\n{}", pf.dump_text());
        assert_eq!(verdicts(&pf, k), ["T0Mono"], "{k}\n{}", pf.dump_text());
        assert_eq!(verdicts(&pf, k), verdicts(&whole, k), "{k}");
    }
    assert!(tmpl(&pf, "Node").prefix_stable && tmpl(&pf, "Node").fixed_layout);
    // The bindings holding Node are the whole solve's: `n` is Node only,
    // `l`/`r` Node or null; a marker load adds no `undefined` (DECISIONS
    // 111), so `n` needs no nullish receiver test.
    let held = |pf: &ProgramFacts| -> Vec<String> {
        pf.bindings[0]
            .locals
            .iter()
            .filter(|(_, b)| !b.class_ids.is_empty())
            .map(|(id, b)| format!("{id} {:?} prims={} other={}", b.class_ids, b.prims, b.other))
            .collect()
    };
    assert!(held(&pf).len() >= 3, "{:?}", held(&pf));
    assert_eq!(held(&pf), held(&whole), "{}", pf.dump_text());
}

#[test]
fn a_read_back_instance_used_beyond_its_contract_breaks_the_class() {
    // Soundness of DECISIONS 110: `poke`'s unit reads `n.left` as Node's
    // stand-in, so adding a key to it is that unit's census report.
    let src = TREE.replace(
        "console.log(check(bottomUp(4)));",
        "function poke(n: Node) { const l: any = n.left; if (l !== null) l.z = 1; }\n\
         const t = bottomUp(3); poke(t); console.log(check(t));",
    );
    let pf = units(&src);
    assert!(
        pf.broken.iter().any(|d| d.ends_with(":Node")),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
    assert!(!verdicts(&pf, "left").iter().all(|v| v.starts_with("T0")));
}

#[test]
fn a_constructor_argument_no_contract_describes_escapes() {
    // Soundness of DECISIONS 109: only another unit's stand-in stays in its
    // contract when handed to the constructor. `leaf`'s unit passes its own
    // object: it escapes there (the defining unit sees it as a layout it
    // can write into), and `left` holds more than Node.
    let src = TREE.replace(
        "console.log(check(bottomUp(4)));",
        "function leaf(): Node { const o: any = { left: null, right: null }; return new Node(o, null); }\n\
         console.log(check(leaf()));",
    );
    let pf = units(&src);
    let o = pf
        .templates
        .iter()
        .find(|t| t.display.contains("__AnonShape") && t.birth_keys == ["left", "right"])
        .unwrap_or_else(|| panic!("the literal in\n{}", pf.dump_text()));
    assert!(o.escape.is_some(), "{}", pf.dump_text());
    assert!(
        !verdicts(&pf, "left").iter().all(|v| v.starts_with("T0")),
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 137: CD's RedBlackTree.put stores `this.root` (what its
/// published receiver reaches) into another unit's Node. The receiver marker
/// has the running units' receivers beside it (DECISIONS 74/107): it is no
/// value of its own, so the store is described by those values, not sunk
/// and published unknown to Node's owner.
const TREE_ROOT: &str = r#"
    class Node {
      parent: Node | null;
      constructor() { this.parent = null; }
    }
    class Tree {
      root: Node | null;
      constructor() { this.root = null; }
      put(): void { const z = new Node(); z.parent = this.root; this.root = z; }
    }
    function walk(t: Tree): number {
      let n = 0;
      let x: Node | null = t.root;
      while (x !== null) { n++; x = x.parent; }
      return n;
    }
    const t = new Tree();
    t.put();
    t.put();
    console.log(walk(t));
    "#;

#[test]
fn a_receiver_reach_stored_into_another_units_instance_keeps_its_class() {
    let pf = units(TREE_ROOT);
    let whole = facts(TREE_ROOT);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let (u, w) = (
        field(tmpl(&pf, "Node"), "parent"),
        field(tmpl(&whole, "Node"), "parent"),
    );
    assert!(!u.kind.top, "{}", pf.dump_text());
    assert_eq!(u.kind.objs, w.kind.objs, "{}", pf.dump_text());
    assert_eq!(
        verdicts(&pf, "parent"),
        verdicts(&whole, "parent"),
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 138: CD's Node.successor returns `treeMinimum(this.right)` or
/// the end of a `.parent` walk: a reach of its receiver, then the keys the
/// body reads on it, repeatedly. The contract says so (not ⊤, not
/// everything the receiver reaches, which would add the tree's values), so
/// another unit's store through the result stays on Node.
const TREE_SUCC: &str = r#"
    class Val { v: number; constructor(v: number) { this.v = v; } }
    function treeMinimum(x: any): any {
      while (x.left) { x = x.left; }
      return x;
    }
    class Node {
      value: Val; left: Node | null; right: Node | null; parent: Node | null;
      constructor(value: Val) { this.value = value; this.left = null; this.right = null; this.parent = null; }
      successor(): Node | null {
        let x: Node = this;
        if (x.right) { return treeMinimum(x.right); }
        let y = x.parent;
        while (y && x === y.right) { x = y; y = y.parent; }
        return y;
      }
    }
    class Tree {
      root: Node | null;
      constructor() { this.root = null; }
      insert(v: number): Node {
        const z = new Node(new Val(v));
        z.parent = this.root;
        if (this.root) { this.root.right = z; } else { this.root = z; }
        return z;
      }
      unlink(): number {
        const z = this.root;
        if (!z) { return 0; }
        const y = z.successor();
        if (y) { y.parent = null; return y.value.v; }
        return 0;
      }
    }
    const t = new Tree();
    t.insert(1);
    t.insert(2);
    console.log(t.unlink());
    "#;

#[test]
fn a_receiver_walk_result_of_another_units_method_keeps_its_class() {
    // Merged head: DECISIONS 191 restores `parent`'s class (whole program).
    let (pf, wf) = (units(TREE_SUCC), facts(TREE_SUCC));
    assert_merged(&pf, &wf, &["t.ts:Node", "t.ts:Val"]);
}

/// DECISIONS 140: CD's RedBlackTree.forEach calls its parameter with an
/// instance of its own unit (`fn(new Entry(..))`). The function is the
/// caller unit's: that unit runs the call on it with the argument the
/// contract describes, as it takes a contract result, so the instance does
/// not escape and the closure's reads stay on Entry. (What Entry's fields
/// hold per unit is DECISIONS 141: its constructor arguments are reaches of
/// the method's receiver, ⊤ to Entry's unit, with or without a callback.)
const TREE_EACH: &str = r#"
    class Val { v: number; constructor(v: number) { this.v = v; } }
    class Entry {
      key: number; value: Val;
      constructor(key: number, value: Val) { this.key = key; this.value = value; }
    }
    class Tree {
      key: number; val: Val;
      constructor() { this.key = 1; this.val = new Val(2); }
      forEach(fn: (e: Entry) => void): void { fn(new Entry(this.key, this.val)); }
    }
    const t = new Tree();
    let s = 0;
    t.forEach((e: Entry) => { s += e.key + e.value.v; });
    console.log(s);
    "#;

#[test]
fn a_call_through_a_parameter_hands_its_argument_to_the_callers_unit() {
    let pf = units(TREE_EACH);
    let whole = facts(TREE_EACH);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert_eq!(
        verdicts(&pf, "value"),
        verdicts(&whole, "value"),
        "{}",
        pf.dump_text()
    );
    assert_eq!(
        verdicts(&pf, "key"),
        verdicts(&whole, "key"),
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 142: a published method's receiver reaches (`this.key`,
/// `this.val`) passed to another unit's constructor are described by the
/// receivers the running units published beside them, not as bare markers.
#[test]
fn a_receiver_reach_passed_to_another_units_constructor_keeps_its_kinds() {
    let src = TREE_EACH
        .replace(
            "forEach(fn: (e: Entry) => void): void { fn(new Entry(this.key, this.val)); }",
            "make(): Entry { return new Entry(this.key, this.val); }",
        )
        .replace(
            "t.forEach((e: Entry) => { s += e.key + e.value.v; });",
            "const e = t.make(); s += e.key + e.value.v;",
        );
    assert!(src.contains("make()") && src.contains("t.make()"));
    let pf = units(&src);
    let whole = facts(&src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["key", "value"] {
        let (u, w) = (
            field(tmpl(&pf, "Entry"), k),
            field(tmpl(&whole, "Entry"), k),
        );
        assert!(!w.kind.top && !u.kind.top, "{k}\n{}", pf.dump_text());
    }
    assert_eq!(
        verdicts(&pf, "v"),
        verdicts(&whole, "v"),
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 143: SOM's `Vector.sort`/`sortRange` recursion passes a
/// receiver field (`this.first`) where the callback goes, so the callee
/// marker of `cmp(..)` stands for the parameter and a receiver reach. The
/// reach has the published receivers beside it: it adds no callee, the call
/// stays a call through the parameter (DECISIONS 140), and the element
/// handed to it keeps its class.
const SORT_RANGE: &str = r#"
    class Val { v: number; constructor(v: number) { this.v = v; } }
    class Box {
      first: number; val: Val;
      constructor() { this.first = 0; this.val = new Val(1); }
      sortRange(i: any, cmp: any): void {
        if (cmp) { cmp(this.val); }
        if (i > 3) { this.sort(i); }
      }
      sort(cmp: any): void { this.sortRange(this.first, cmp); }
    }
    const b = new Box();
    let s = 0;
    b.sort((x: Val) => { s += x.v; });
    console.log(s);
    "#;

#[test]
fn a_call_through_a_parameter_that_may_hold_a_receiver_field_keeps_its_argument() {
    let pf = units(SORT_RANGE);
    let whole = facts(SORT_RANGE);
    // No call of an unknown function value: `cmp(this.val)` stays a call
    // through the parameter (a receiver reach member escaped it before).
    assert_eq!(pf.stats.top_calls, 0, "{}", pf.dump_text());
    assert!(
        !pf.broken.iter().any(|b| b.contains("Val")),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
    assert_eq!(
        verdicts(&pf, "v"),
        verdicts(&whole, "v"),
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 140, soundness: what the caller's function does to the
/// argument is that unit's census on the stand-in, reported to the owner
/// (§11.3): a store of another kind reaches Entry's facts.
#[test]
fn a_store_into_an_argument_handed_through_a_parameter_reaches_its_owner() {
    let src = TREE_EACH.replace(
        "s += e.key + e.value.v;",
        "s += e.key; e.value = null as any;",
    );
    let pf = units(&src);
    let whole = facts(&src);
    let broken = pf.broken.iter().any(|b| b.contains("Entry"));
    assert!(
        broken || verdicts(&pf, "value") == verdicts(&whole, "value"),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
    let (u, w) = (
        field(tmpl(&pf, "Entry"), "value"),
        field(tmpl(&whole, "Entry"), "value"),
    );
    // A test must be able to fail (DECISIONS 142 lit it): Entry's fields
    // are described per unit now, so a ⊤ field would hide a missing store.
    assert!(broken || !u.kind.top, "{}", pf.dump_text());
    assert!(
        broken || u.kind.prims & w.kind.prims == w.kind.prims,
        "{}",
        pf.dump_text()
    );
}

/// DECISIONS 144: CD's `reduceCollisionSet(motions)` calls a method of its
/// parameter (`motions.forEach(cb)`), RedBlackTree.put calls one of its key
/// (`key.compareTo(k)`). The value is the caller unit's: that unit runs the
/// method on what it passed, with the arguments the contract describes, as
/// it runs a call through a parameter (DECISIONS 140), so the instance does
/// not escape and the callback's reads stay on the class.
const PARAM_MCALL: &str = r#"
    class Val { v: number; constructor(v: number) { this.v = v; } }
    class Coll {
      val: Val;
      constructor() { this.val = new Val(1); }
      each(fn: (x: Val) => void): void { fn(this.val); }
    }
    class Key {
      k: number;
      constructor(k: number) { this.k = k; }
      cmp(o: Key): number { return this.k - o.k; }
    }
    class Det {
      reduce(c: Coll): number { let s = 0; c.each((x: Val) => { s += x.v; }); return s; }
      put(key: Key, other: Key): number { return key.cmp(other); }
    }
    const d = new Det();
    console.log(d.reduce(new Coll()) + d.put(new Key(1), new Key(2)));
    "#;

#[test]
fn a_method_call_on_a_parameter_runs_in_the_callers_unit() {
    // Merged head: DECISIONS 190 restores Val's verdicts (whole program).
    let (pf, wf) = (units(PARAM_MCALL), facts(PARAM_MCALL));
    assert_merged(&pf, &wf, &["t.ts:Val"]);
}

/// DECISIONS 147: a function a contract function hands the caller's unit
/// (here its result) is a contract function of its defining unit: the
/// caller calls it through that contract, so the instance it passes is
/// published to the defining unit instead of escaping into unknown code.
const RESULT_FN: &str = r#"
    class Val { v: number; constructor(v: number) { this.v = v; } }
    class Det {
      reader(): (x: Val) => Val { return (x: Val) => new Val(x.v + 1); }
    }
    const f = new Det().reader();
    console.log(f(new Val(1)).v);
    "#;

#[test]
fn a_function_handed_to_another_unit_is_called_through_its_contract() {
    let pf = units(RESULT_FN);
    let whole = facts(RESULT_FN);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    // `x.v` reads the handed-out function's parameter: Val beside ⊤ per unit
    // (E0) vs T0Mono whole, DECISIONS 148 (open); without 147 Val breaks.
    assert!(!whole.broken.iter().any(|b| b.contains("Val")));
    // A test must be able to fail: a reader that drops the described
    // function calls nothing, so `f(..).v` in the caller's unit sees no
    // receiver (`x.v` alone cannot tell: its conservative entry holds Val).
    let vs: Vec<_> = pf.sites.iter().filter(|s| s.key == "v").collect();
    assert!(
        vs.len() == 2 && vs.iter().all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
}

/// The storage a method's receiver reaches: made by the constructor (`D`),
/// or stored by the caller (`C`).
const APPEND_OWN: &str = r#"
    class Vec {
      storage: any[]; n: number;
      constructor() { this.storage = new Array(4); this.n = 0; }
      append(e: any) { this.storage[this.n] = e; this.n = this.n + 1; }
    }
    class P { x: number; constructor(x: number) { this.x = x; } }
    function main() { const v = new Vec(); v.append(new P(1)); console.log(v.storage[0].x); }
    main();
    "#;
const APPEND_SET: &str = r#"
    class Vec {
      items: any; n: number;
      constructor() { this.items = null; this.n = 0; }
      append(e: any) { this.items[this.n] = e; this.n = this.n + 1; }
    }
    class P { x: number; constructor(x: number) { this.x = x; } }
    function main() { const v = new Vec(); v.items = []; v.append(new P(1)); console.log(v.items[0].x); }
    main();
    "#;
/// The caller reads its own array the constructor was handed (`B2`): its
/// elements are what Vec's unit stores there, by the element cell.
const APPEND_ALIAS: &str = r#"
    class Vec {
      items: any[]; n: number;
      constructor(a: any[]) { this.items = a; this.n = 0; }
      append(e: any) { this.items[this.n] = e; this.n = this.n + 1; }
    }
    class P { x: number; constructor(x: number) { this.x = x; } }
    function main() { const a: any[] = []; const v = new Vec(a); v.append(new P(1)); console.log(a[0].x); }
    main();
    "#;
/// A constructor filling the array it is handed, keeping no field of it.
const FILL_ARG: &str = r#"
    class P { x: number; constructor(x: number) { this.x = x; } }
    class Filler { n: number; constructor(a: any[]) { a[0] = new P(1); this.n = 1; } }
    function main() { const a: any[] = []; const f = new Filler(a); console.log(a[0].x, f.n); }
    main();
    "#;
/// The storage handed to the constructor by the caller (`B`).
const APPEND_ARG: &str = r#"
    class Vec {
      items: any[]; n: number;
      constructor(a: any[]) { this.items = a; this.n = 0; }
      append(e: any) { this.items[this.n] = e; this.n = this.n + 1; }
    }
    class P { x: number; constructor(x: number) { this.x = x; } }
    function main() { const v = new Vec([]); v.append(new P(1)); console.log(v.items[0].x); }
    main();
    "#;

#[test]
fn an_argument_stored_into_what_the_receiver_reaches_stays_the_callers() {
    // DECISIONS 149, 149b, 149d: `this.storage[n] = e` stores the caller's
    // argument into the caller's view of the storage, as the whole program
    // runs it: append's parameter is its receiver flow, which `main`
    // replays on its stand-in's storage, not an escape in Vec's unit (the
    // storage there is published: its elements come back from `main`).
    // DECISIONS 149e: the caller's own array handed to the closed
    // constructor leaves by its element cell and comes back to the caller
    // as that array, so the replay lands on it.
    // Per-unit facts are whole-program facts (owner 2026-10-23).
    for src in [APPEND_OWN, APPEND_SET, APPEND_ARG] {
        let pf = units(src);
        let w = verdicts(&facts(src), "x");
        assert!(pf.broken.is_empty(), "{}", pf.dump_text());
        assert!(
            !w.is_empty() && w.iter().all(|v| v.starts_with("T0")),
            "{w:?}"
        );
        assert_eq!(verdicts(&pf, "x"), w, "{}", pf.dump_text());
    }
    // DECISIONS 149e: the caller's own array handed to the closed
    // constructor is the array Vec's unit stores into: the caller reads
    // what append stored there through the element cell. (The whole
    // program does not see `P` at `a[0]` today, an open item of 149e, so
    // this asserts the per-unit fact itself.)
    // The same when the constructor keeps no field of it (`FILL_ARG`):
    // the array is handed over by its element cell itself.
    for src in [APPEND_ALIAS, FILL_ARG] {
        let pf = units(src);
        assert!(pf.broken.is_empty(), "{}", pf.dump_text());
        let u = verdicts(&pf, "x");
        assert!(
            !u.is_empty() && u.iter().all(|v| v.starts_with("T0")),
            "{u:?}"
        );
    }
}

/// DECISIONS 151: an in-unit call handed what the receiver reaches, whose
/// result is stored back into that reach.
const REACH_BACK: &str = r#"
    class Node { left: any; right: any; constructor() { this.left = null; this.right = null; } }
    function pick(n: any): any { return n; }
    class Tree {
      root: any;
      constructor() { this.root = new Node(); this.root.right = new Node(); }
      rot() { const z = this.root; const x = pick(z.right); z.left = x; }
    }
    function main() { const t = new Tree(); t.rot(); console.log(t.root.left.right); }
    main();
    "#;
const REACH_BACK_M: &str = r#"
    class Node { left: any; right: any; constructor() { this.left = null; this.right = null; } }
    class Tree {
      root: any;
      constructor() { this.root = new Node(); this.root.right = new Node(); }
      pick(n: any): any { return n; }
      fix(x: any, p: any) { if (p) { p.left = x; } }
      rot() { const z = this.root; const x = this.pick(z.right); z.left = x; this.fix(z.right, z); }
    }
    function main() { const t = new Tree(); t.rot(); console.log(t.root.left.right); }
    main();
    "#;

/// DECISIONS 153: a returned list whose link holds another unit's
/// instance beside the list's own literal.
const LOOP_LIST: &str = r#"
    class Ext { v: number; constructor() { this.v = 1; } }
    function mk(n: number): any { let h: any = new Ext(); for (let i = 0; i < n; i++) { h = { next: h }; } return h; }
    function main() { const h = mk(3); console.log(h.next.next.v); }
    main();
    "#;

#[test]
fn a_receiver_reach_handed_to_an_in_unit_call_and_stored_back_stays_the_callers() {
    // DECISIONS 151: `x = pick(z.right); z.left = x` stores what an
    // in-unit call hands back, the receiver's own reach, into that reach:
    // the receiver flow the reader replays on its own objects, as a value
    // stored directly is (DECISIONS 149), not an escape that breaks Node.
    // Per-unit facts are whole-program facts (owner 2026-10-23).
    for src in [REACH_BACK, REACH_BACK_M] {
        let pf = units(src);
        let wf = facts(src);
        assert!(pf.broken.is_empty(), "{}", pf.dump_text());
        for k in ["left", "right"] {
            let w = verdicts(&wf, k);
            assert!(
                !w.is_empty() && w.iter().all(|v| v.starts_with("T0")),
                "{w:?}"
            );
            assert_eq!(verdicts(&pf, k), w, "{}", pf.dump_text());
        }
    }
}

#[test]
fn a_layout_whose_link_holds_it_beside_another_units_instance_is_described_once() {
    // Merged head: nothing breaks; `h.next` reads ⊤ per unit where the whole
    // program has its layout (no proof either way): DECISIONS 190.
    let (pf, wf) = (units(LOOP_LIST), facts(LOOP_LIST));
    assert_merged(&pf, &wf, &[]);
    assert_eq!(
        verdicts(&pf, "next"),
        ["No(\"receiver from unmodeled code (TOP)\")"]
    );
    assert_eq!(
        verdicts(&wf, "next"),
        ["No(\"key not a definite birth field\")"]
    );
}

/// DECISIONS 152: a closed class's constructor handed a value of the
/// constructing unit (the caller's argument, a contract marker): CD's
/// RedBlackTree.treeInsert `new Node(key, value)`.
const CTOR_MARK: &str = r#"
    class Key { x: number; constructor(x: number) { this.x = x; } cmp(o: number): number { return this.x - o; } }
    class Node { k: any; left: any; constructor(k: any) { this.k = k; this.left = null; } }
    class Tree {
      root: any;
      constructor() { this.root = null; }
      insert(k: any) {
        const z = new Node(k);
        if (this.root === null) { this.root = z; } else if (k.cmp(this.root.k.x) < 0) { this.root.left = z; }
      }
    }
    function main() { const t = new Tree(); t.insert(new Key(2)); t.insert(new Key(1)); console.log(t.root.k.x + t.root.left.k.x); }
    main();
    "#;

#[test]
fn a_constructor_argument_of_the_constructing_unit_stays_the_callers() {
    // DECISIONS 152: `new Node(k)` hands Tree's parameter (the caller's
    // value) to the closed class's constructor. Node's unit publishes what
    // its constructor does with each argument (its function contract:
    // `this.k = k`), so Tree's unit replays that on its own marker instead
    // of letting it escape, and `t.root.k.x` keeps Key.
    // Per-unit facts are whole-program facts (owner 2026-10-23).
    let pf = units(CTOR_MARK);
    let wf = facts(CTOR_MARK);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["k", "x", "left"] {
        let w = verdicts(&wf, k);
        assert!(
            !w.is_empty() && w.iter().all(|v| v.starts_with("T0")),
            "{k} {w:?}"
        );
        assert_eq!(verdicts(&pf, k), w, "{k}\n{}", pf.dump_text());
    }
}

/// DECISIONS 152: what the parameter reaches handed to the constructor.
const CTOR_REACH: &str = r#"
    class Inner { y: number; constructor(y: number) { this.y = y; } }
    class Key { x: number; inner: any; constructor(x: number) { this.x = x; this.inner = new Inner(x); } cmp(o: number): number { return this.x - o; } }
    class Node { k: any; left: any; constructor(k: any) { this.k = k; this.left = null; } }
    class Tree {
      root: any;
      constructor() { this.root = null; }
      insert(k: any) {
        const z = new Node(k.inner);
        if (this.root === null) { this.root = z; } else if (k.cmp(this.root.k.y) < 0) { this.root.left = z; }
      }
    }
    function main() { const t = new Tree(); t.insert(new Key(2)); t.insert(new Key(1)); console.log(t.root.k.y + t.root.left.k.y); }
    main();
    "#;

#[test]
fn a_constructor_argument_read_from_a_parameter_is_not_the_parameter() {
    // Soundness of DECISIONS 152: `new Node(k.inner)` hands over what the
    // parameter reaches, not the parameter. Its keyed reach marker shares
    // the parameter's entry; were it handed over as the parameter, Node's
    // unit would see Key in `k`, where the program stores Inner.
    let pf = units(CTOR_REACH);
    let k = &field(tmpl(&pf, "Node"), "k").kind;
    assert!(
        !k.objs.iter().any(|o| o.ends_with(":Key")),
        "{k:?}\n{}",
        pf.dump_text()
    );
    assert!(
        k.top || k.objs.iter().any(|o| o.ends_with(":Inner")),
        "{k:?}\n{}",
        pf.dump_text()
    );
}

/// DECISIONS 155: what the receiver reaches handed to a closed class's
/// constructor: CD's RedBlackTree.treeInsert `new InsertResult(false, null,
/// x.value)` with `x` read down `this.root` / `x.left`.
const RECV_REACH: &str = r#"
    class Val { n: number; constructor(n: number) { this.n = n; } }
    class Node { value: any; left: any; constructor(v: any) { this.value = v; this.left = null; } }
    class Res { v: any; constructor(v: any) { this.v = v; } }
    class Tree {
      root: any;
      constructor() { this.root = null; }
      insert(v: any): any {
        let x = this.root;
        while (x !== null && x.left !== null) { x = x.left; }
        if (x !== null && x.value.n === 2) { return new Res(x.value); }
        const z = new Node(v);
        if (x === null) { this.root = z; } else { x.left = z; }
        return new Res(null);
      }
    }
    function main() {
      const t = new Tree(); t.insert(new Val(1)); t.insert(new Val(2));
      const r = t.insert(new Val(2)); console.log(r.v.n + t.root.left.value.n);
    }
    main();
    "#;

#[test]
fn a_constructor_argument_the_receiver_reaches_stays_the_callers() {
    // DECISIONS 155: `new Res(x.value)` hands what the receiver reaches
    // (its keyed reach marker under `root`) to Res's constructor. The
    // receivers the caller's unit runs `insert` on are published beside the
    // marker (DECISIONS 107): they reach the constructor themselves, path
    // by path, so the marker does not escape (it stood for no value of its
    // own), Node and Val stay whole and `r.v.n` keeps Val.
    // Per-unit facts are whole-program facts (owner 2026-10-23).
    let pf = units(RECV_REACH);
    let wf = facts(RECV_REACH);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["v", "n", "value", "left"] {
        let w = verdicts(&wf, k);
        assert!(
            !w.is_empty() && w.iter().all(|v| v.starts_with("T0")),
            "{k} {w:?}"
        );
        assert_eq!(verdicts(&pf, k), w, "{k}\n{}", pf.dump_text());
    }
    // Soundness: Res's unit sees the caller's Val in `v`, as the program.
    let (u, w) = (field(tmpl(&pf, "Res"), "v"), field(tmpl(&wf, "Res"), "v"));
    assert_eq!(u.kind, w.kind, "{}", pf.dump_text());
}

const PUB_CTOR: &str = r#"
    class Box { k: number; constructor(k: number) { this.k = k; } }
    class Vec {
      cap: any;
      static tag = 1;
      constructor(n: any) { this.cap = n; }
    }
    function main() {
      const v = new Vec(new Box(3));
      const w = new Vec(new Box(4));
      console.log(v.cap.k + w.cap.k + Vec.tag);
    }
    main();
    "#;

#[test]
fn a_published_class_takes_its_constructions_from_the_units_holding_it() {
    // DECISIONS 157: Vec has a static field, so it is no closed class
    // (§18.13) and its unit published it with ⊤ constructor arguments.
    // Every unit holding it holds its stand-in, whose `new` publishes what
    // it passes: `cap` holds Box as in the whole program, and `.k` proves.
    // Per-unit facts are whole-program facts (owner 2026-10-23).
    let pf = units(PUB_CTOR);
    let wf = facts(PUB_CTOR);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let w = verdicts(&wf, "k");
    assert!(
        !w.is_empty() && w.iter().all(|v| v.starts_with("T0")),
        "{w:?}"
    );
    assert_eq!(verdicts(&pf, "k"), w, "{}", pf.dump_text());
    let (u, w) = (
        field(tmpl(&pf, "Vec"), "cap"),
        field(tmpl(&wf, "Vec"), "cap"),
    );
    assert_eq!(u.kind, w.kind, "{}", pf.dump_text());
}

const PUB_CTOR_ESCAPE: &str = r#"
    class Box { k: number; constructor(k: number) { this.k = k; } }
    class Vec {
      cap: any;
      static tag = 1;
      constructor(n: any) { this.cap = n; }
    }
    function main() {
      const v = new Vec(new Box(3));
      (globalThis as any).ext(Vec);
      console.log(v.cap.k);
    }
    main();
    "#;

#[test]
fn a_published_class_another_unit_lets_escape_has_unknown_constructor_arguments() {
    // DECISIONS 157, the census guard: `main`'s unit hands Vec's stand-in to
    // unknown code, which may construct it with anything: its report breaks
    // Vec, and Vec's unit runs the constructor with ⊤ arguments again.
    // The fixture has no static method: one taking a parameter into
    // `new Vec` would make `cap` ⊤ on its own and hide the guard.
    let pf = units(PUB_CTOR_ESCAPE);
    assert!(
        pf.broken.iter().any(|b| b.ends_with(":Vec")),
        "{:?}",
        pf.broken
    );
    let u = field(tmpl(&pf, "Vec"), "cap");
    assert!(u.kind.top, "{:?}\n{}", u.kind, pf.dump_text());
}

const ELEM_READ: &str = r#"
    class Vec {
      storage: any; n: number;
      static tag = 1;
      constructor() { this.storage = null; this.n = 0; }
      append(e: any) { if (this.storage === null) { this.storage = new Array(4); } this.storage[this.n] = e; this.n += 1; }
      forEach(fn: any) { for (let i = 0; i < this.n; i += 1) { fn(this.storage[i]); } }
      size(): number { return this.n; }
    }
    class Item { k: number; constructor(k: number) { this.k = k; } }
    function group(): Vec { const r = new Vec(); const a = new Vec(); a.append(new Item(1)); r.append(a); return r; }
    function main() {
      const all = group(); let s = 0;
      all.forEach((v: any) => { for (let i = 0; i < v.size(); i += 1) { s += 1; } });
      console.log(s + Vec.tag);
    }
    main();
    "#;

#[test]
fn an_element_a_callee_reads_is_the_callers_element() {
    // Merged head: DECISIONS 190 restores the callback's element (whole program).
    let (pf, wf) = (units(ELEM_READ), facts(ELEM_READ));
    assert_merged(&pf, &wf, &["t.ts:Item", "t.ts:Vec"]);
}

const SUB_RECV: &str = r#"
    class Vec {
      storage: any; n: number;
      static tag = 1;
      constructor() { this.storage = null; this.n = 0; }
      append(e: any) { if (this.storage === null) { this.storage = new Array(4); } this.storage[this.n] = e; this.n += 1; }
      hasSome(fn: any): boolean { for (let i = 0; i < this.n; i += 1) { if (fn(this.storage[i])) { return true; } } return false; }
    }
    class Bag {
      items: Vec;
      static tag = 2;
      constructor() { this.items = new Vec(); }
      add(e: any) { this.items.append(e); }
      hasSome(fn: any): boolean { return this.items.hasSome(fn); }
      contains(o: any): boolean { return this.hasSome((e: any) => e === o); }
    }
    class IdBag extends Bag {
      contains(o: any): boolean { return this.hasSome((e: any) => e === o); }
    }
    class Item { k: number; constructor(k: number) { this.k = k; } }
    class Tag { k: number; t: number; constructor(t: number) { this.k = 0; this.t = t; } }
    function main() {
      const b = new Bag(); b.add(new Tag(3));
      const it = new Item(1); const v = new Vec(); v.append(it);
      const e: any = v.storage[0];
      console.log(b.contains(it) ? 1 : 0, e.k + Vec.tag + Bag.tag);
    }
    main();
    "#;

#[test]
fn an_argument_a_callee_stores_into_a_shared_array_is_stored_by_the_callers_unit() {
    // Merged head: DECISIONS 190 restores `e.k` on Item and Tag (whole program).
    let (pf, wf) = (units(SUB_RECV), facts(SUB_RECV));
    assert_merged(
        &pf,
        &wf,
        &[
            "override contains of t.ts:Bag",
            "override items of t.ts:Bag",
            "t.ts:Item",
            "t.ts:Tag",
        ],
    );
}

const ROOT_BACK: &str = r#"
    class N { k: number; left: any; right: any; static tag = 1;
      constructor(k: number) { this.k = k; this.left = null; this.right = null; } }
    class Tree { root: any; static tag = 2;
      constructor() { this.root = null; }
      put(k: number) { const z = new N(k); if (this.root === null) { this.root = z; } else { z.right = this.root; this.root = z; this.rot(this.root); } }
      rot(x: any) { const y = x.right; if (y !== null) { x.right = y.left; y.left = x; if (x === this.root) { this.root = y; } } }
      sum(): number { let s = 0; let x = this.root; while (x !== null) { s += x.k; x = x.left; } return s; }
    }
    function main() { const t = new Tree(); t.put(1); t.put(2); t.put(3); console.log(t.sum() + N.tag + Tree.tag); }
    main();
    "#;

#[test]
fn a_receiver_reach_stored_back_into_a_leaving_field_adds_no_kinds() {
    // DECISIONS 164: `Tree.rot` stores what it read through its argument
    // (`this.root` passed by `put`) back into its receiver's `root`: the
    // argument marker's reach and the receiver reach marker, beside the
    // receivers main publishes (DECISIONS 107), land in the field of the
    // leaving Tree. They stand for no value of their own (DECISIONS 74):
    // the field is described by the N instances beside them, as the whole
    // program sees it. Sunk, N broke (the AWFY CD RedBlackTree.root shape:
    // Node escaped, its class went unknown and every `new Node` was ⊤).
    let pf = units(ROOT_BACK);
    let wf = facts(ROOT_BACK);
    assert!(
        pf.broken
            .iter()
            .all(|b| b.starts_with("override ") || wf.broken.contains(b)),
        "{:?} vs {:?}\n{}",
        pf.broken,
        wf.broken,
        pf.dump_text()
    );
    // Per-unit facts are whole-program facts (owner 2026-10-23): every site
    // equal, the reads inside `rot` through its own parameter included
    // (DECISIONS 165: no other unit runs `rot`, so nothing but what `put`
    // passes is beside its parameter marker).
    let v = |f: &ProgramFacts| -> Vec<String> {
        let mut v: Vec<String> = f
            .sites
            .iter()
            .map(|s| {
                let mut r = s.receivers.clone();
                r.sort();
                format!("@{} .{} {:?} {:?}", s.byte_offset, s.key, s.verdict, r)
            })
            .collect();
        v.sort();
        v
    };
    assert!(
        v(&wf).iter().any(|s| s.contains(".k T0Mono")),
        "{:?}",
        v(&wf)
    );
    assert_eq!(v(&pf), v(&wf), "\n{}", pf.dump_text());
}

const TWO_FN_KEY: &str = r#"
    class N { v: number; constructor(v: number) { this.v = v; } }
    class R { w: number; v: number; constructor(v: number) { this.w = 1; this.v = v; } }
    class C {
      constructor() { if (Math.random() > 2) { (C.prototype as any).m = function (y: any): number { return 0; }; } }
      m(x: any): number { return x.v; }
      own(): number { return this.m(new N(1)); }
    }
    function main() { const c = new C(); console.log(c.own() + c.m(new R(2))); }
    main();
    "#;

#[test]
fn a_method_its_readers_cannot_name_is_run_by_its_owner() {
    // DECISIONS 165: `C.prototype.m` holds two functions, so C's class
    // contract lists no `m` (a method is named alone under its key) and
    // main calls it through ⊤, publishing no run. C's unit publishes `m`
    // as run itself: its parameter marker still stands for main's R, so
    // `x.v` is not proven on N alone (it would be, wrongly, if `m` read as
    // unrun: nothing but `own`'s N beside its marker).
    let pf = units(TWO_FN_KEY);
    let wf = facts(TWO_FN_KEY);
    let at = TWO_FN_KEY.find("m(x").unwrap() as u32..TWO_FN_KEY.find("own()").unwrap() as u32;
    let site = |f: &ProgramFacts| -> String {
        let s = f
            .sites
            .iter()
            .find(|s| at.contains(&s.byte_offset) && s.key == "v")
            .expect("x.v site");
        let mut r = s.receivers.clone();
        r.sort();
        format!("{:?} {:?}", s.verdict, r)
    };
    assert!(site(&wf).contains("R"), "{}", site(&wf));
    let u = site(&pf);
    assert!(
        u.contains("R") || !u.contains("T0"),
        "{u}\n{}",
        pf.dump_text()
    );
}
