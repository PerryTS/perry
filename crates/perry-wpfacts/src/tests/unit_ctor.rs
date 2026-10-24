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
    let (u, w) = (field(tmpl(&pf, "Node"), "parent"), field(tmpl(&whole, "Node"), "parent"));
    assert!(!u.kind.top, "{}", pf.dump_text());
    assert_eq!(u.kind.objs, w.kind.objs, "{}", pf.dump_text());
    assert_eq!(verdicts(&pf, "parent"), verdicts(&whole, "parent"), "{}", pf.dump_text());
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
    let pf = units(TREE_SUCC);
    let whole = facts(TREE_SUCC);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let (u, w) = (field(tmpl(&pf, "Node"), "parent"), field(tmpl(&whole, "Node"), "parent"));
    assert!(!u.kind.top, "{}", pf.dump_text());
    assert_eq!(u.kind.objs, w.kind.objs, "{}", pf.dump_text());
    assert_eq!(verdicts(&pf, "parent"), verdicts(&whole, "parent"), "{}", pf.dump_text());
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
    assert_eq!(verdicts(&pf, "value"), verdicts(&whole, "value"), "{}", pf.dump_text());
    assert_eq!(verdicts(&pf, "key"), verdicts(&whole, "key"), "{}", pf.dump_text());
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
        let (u, w) = (field(tmpl(&pf, "Entry"), k), field(tmpl(&whole, "Entry"), k));
        assert!(!w.kind.top && !u.kind.top, "{k}\n{}", pf.dump_text());
    }
    assert_eq!(verdicts(&pf, "v"), verdicts(&whole, "v"), "{}", pf.dump_text());
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
    assert!(!pf.broken.iter().any(|b| b.contains("Val")), "{:?}\n{}", pf.broken, pf.dump_text());
    assert_eq!(verdicts(&pf, "v"), verdicts(&whole, "v"), "{}", pf.dump_text());
}

/// DECISIONS 140, soundness: what the caller's function does to the
/// argument is that unit's census on the stand-in, reported to the owner
/// (§11.3): a store of another kind reaches Entry's facts.
#[test]
fn a_store_into_an_argument_handed_through_a_parameter_reaches_its_owner() {
    let src = TREE_EACH.replace("s += e.key + e.value.v;", "s += e.key; e.value = null as any;");
    let pf = units(&src);
    let whole = facts(&src);
    let broken = pf.broken.iter().any(|b| b.contains("Entry"));
    assert!(
        broken || verdicts(&pf, "value") == verdicts(&whole, "value"),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
    let (u, w) = (field(tmpl(&pf, "Entry"), "value"), field(tmpl(&whole, "Entry"), "value"));
    // A test must be able to fail (DECISIONS 142 lit it): Entry's fields
    // are described per unit now, so a ⊤ field would hide a missing store.
    assert!(broken || !u.kind.top, "{}", pf.dump_text());
    assert!(broken || u.kind.prims & w.kind.prims == w.kind.prims, "{}", pf.dump_text());
}
