//! BATCHB26: the objects of one `JSON.parse` result are distinct objects
//! (ECMA-262 25.5.1: every object and array of the result is fresh; no
//! object sits at two positions), so a program write into one position is
//! not read from another (`solve::json_pos`). three.js skin: `lightDef.spot
//! = lightDef.spot || {}` (GLTFLoader `_loadLight`) was read back as a key of
//! `objectDef.extensions` (`addUnknownExtensionsToUserData`).
//! Controls stay merged/open: an unknown key reaches every child, an alias
//! joins, a for-in over the parent reaches the named child, a tree handed
//! to unknown code escapes, a recursive walker terminates and reaches the
//! children it walks.
use super::*;

const CLASSES: &str = "class M { go() { return 1; } }\nclass S { go() { return 2; } }\nconst j: any = JSON.parse(process.argv[2]);\n";

/// The `.go` call sites hold a receiver of class `c` (or ⊤).
fn reaches(src: &str, c: &str) -> bool {
    let pf = facts(&format!("{CLASSES}{src}"));
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "go" && s.is_call).collect();
    assert!(!v.is_empty(), "no .go call\n{}", pf.dump_text());
    v.iter().any(|s| s.top || s.receivers.iter().any(|r| r.ends_with(&format!(":{c}"))))
}

#[test]
fn a_store_into_one_named_child_is_not_read_from_another() {
    assert!(!reaches("j.a.x = new M();\nj.b.x.go();", "M"));
    assert!(reaches("j.a.x = new M();\nj.a.x.go();", "M"), "the same child");
}

#[test]
fn control_an_unknown_key_reaches_every_child() {
    assert!(reaches("j[process.argv[3]].x = new M();\nj.b.x.go();", "M"));
}

#[test]
fn control_an_alias_joins() {
    assert!(reaches("j.b = j.a;\nj.a.x = new M();\nj.b.x.go();", "M"));
}

#[test]
fn control_a_for_in_store_reaches_the_named_child() {
    assert!(reaches("for (const k in j) j[k].x = new M();\nj.a.x.go();", "M"));
}

#[test]
fn control_a_tree_given_to_unknown_code_escapes() {
    assert!(reaches("new Function(process.argv[3])(j);\nj.b.x.go();", "M"));
}

/// GLTFLoader.js:732 and :2341.
#[test]
fn skin_light_spot_is_no_extension_of_a_node() {
    let src = "const i = +process.argv[3];\nconst n = +process.argv[4];\nj.lights[i].spot = j.lights[i].spot || new S();\nconst out: any = {};\nfor (const name in j.nodes[n].extensions) out[name] = j.nodes[n].extensions[name];\nout[process.argv[5]].go();";
    assert!(!reaches(src, "S"));
}

#[test]
fn elements_of_one_array_share_a_class_and_other_arrays_do_not() {
    assert!(reaches("j.list[0].tag = new M();\nj.list[1].tag.go();", "M"), "control: one element class");
    assert!(!reaches("j.list[0].tag = new M();\nj.other[0].tag.go();", "M"));
}

#[test]
fn a_recursive_walker_terminates_and_keeps_siblings_apart() {
    let walk = "function w(n: any) { n.x = new M(); if (n.kids) for (const c of n.kids) w(c); }\nw(j);\n";
    assert!(reaches(&format!("{walk}j.kids[0].kids[1].x.go();"), "M"));
    assert!(!reaches(&format!("{walk}j.other.x.go();"), "M"));
}

/// BATCHB27 FOLD: a class is keyed by the last two steps of its path, so
/// `j.x.a.c` and `j.y.a.c` (and `j.a.c`) are one class; `j.b.c` is not.
#[test]
fn the_fold_merges_paths_that_end_in_the_same_two_steps() {
    assert!(reaches("j.x.a.c.t = new M();\nj.y.a.c.t.go();", "M"), "fold: one class for ..a.c");
    assert!(!reaches("j.a.c.t = new M();\nj.b.c.t.go();", "M"), "last two steps differ");
}

/// The element classes of `lights` and `nodes` stay apart at any depth.
#[test]
fn lights_elements_and_nodes_elements_stay_apart() {
    assert!(!reaches("j.lights[0].t = new M();\nj.nodes[0].t.go();", "M"));
    assert!(!reaches("j.ext.lights[0].t = new M();\nj.nodes[1].t.go();", "M"));
    assert!(reaches("j.ext.lights[0].t = new M();\nj.lights[1].t.go();", "M"), "fold: one ..lights[] class");
}
