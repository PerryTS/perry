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
