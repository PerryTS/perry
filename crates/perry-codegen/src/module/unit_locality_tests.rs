//! The unit partition as a function: what it keeps together, how little of it
//! an edit moves, that it is deterministic, and how far from balanced it gets.

use super::*;
use std::collections::HashMap;

/// A module shaped like a library bundle: shared utilities first, then
/// classes of six methods with a private helper, a recursion pair, a call of
/// a shared utility and a closure, and at the end an initializer that names
/// every method.
#[derive(Clone)]
struct Fixture {
    names: Vec<String>,
    bytes: Vec<usize>,
    edges: Vec<UnitEdge>,
}

const UTILITIES: usize = 5;
const METHODS: usize = 6;

impl Fixture {
    fn library(classes: usize) -> Self {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut size = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            2_000 + (seed % 38_000) as usize
        };
        let mut f = Fixture {
            names: Vec::new(),
            bytes: Vec::new(),
            edges: Vec::new(),
        };
        for u in 0..UTILITIES {
            f.names.push(format!("lib__util{u}"));
            f.bytes.push(size());
        }
        let mut closures = 0;
        for c in 0..classes {
            let base = f.names.len() as u32;
            for m in 0..METHODS {
                f.names.push(format!("perry_method_lib__C{c}__m{m}"));
                f.bytes.push(size());
            }
            f.names.push(format!("perry_closure_lib__{closures}"));
            f.bytes.push(size());
            closures += 1;
            let call = |from, to| UnitEdge {
                from,
                to,
                direct: true,
            };
            // m0 -> m1 (m1 has one caller); m1 <-> m2; m3 -> a shared utility;
            // m4 names the closure; m5 stands alone.
            f.edges.push(call(base, base + 1));
            f.edges.push(call(base + 1, base + 2));
            f.edges.push(call(base + 2, base + 1));
            f.edges.push(call(base + 3, (c % UTILITIES) as u32));
            f.edges.push(UnitEdge {
                from: base + 4,
                to: base + METHODS as u32,
                direct: false,
            });
        }
        let init = f.names.len() as u32;
        f.names.push("lib__init_body".to_string());
        f.bytes.push(30_000);
        for to in UTILITIES as u32..init {
            f.edges.push(UnitEdge {
                from: init,
                to,
                direct: false,
            });
        }
        f
    }

    fn nodes(&self) -> Vec<UnitNode<'_>> {
        self.names
            .iter()
            .zip(&self.bytes)
            .map(|(name, &bytes)| UnitNode { name, bytes })
            .collect()
    }

    fn index(&self, name: &str) -> usize {
        self.names.iter().position(|n| n == name).unwrap()
    }

    fn total(&self) -> usize {
        self.bytes.iter().sum()
    }

    /// A new function nothing calls, defined before `at`; the initializer
    /// names it and grows by the registration.
    fn with_inserted(&self, at: usize, name: &str, bytes: usize) -> Self {
        let mut f = self.clone();
        f.names.insert(at, name.to_string());
        f.bytes.insert(at, bytes);
        for e in &mut f.edges {
            if e.from as usize >= at {
                e.from += 1;
            }
            if e.to as usize >= at {
                e.to += 1;
            }
        }
        let init = f.index("lib__init_body");
        f.bytes[init] += 300;
        f.edges.push(UnitEdge {
            from: init as u32,
            to: at as u32,
            direct: false,
        });
        f
    }

    fn locality(&self, n: usize) -> HashMap<String, usize> {
        self.by_name(&units_by_locality(&self.nodes(), &self.edges, n))
    }

    fn size(&self, n: usize) -> HashMap<String, usize> {
        self.by_name(&units_by_size(&self.bytes, n))
    }

    fn by_name(&self, units: &[Vec<usize>]) -> HashMap<String, usize> {
        let mut seen = vec![false; self.names.len()];
        let mut out = HashMap::new();
        for (u, unit) in units.iter().enumerate() {
            assert!(!unit.is_empty(), "unit {u} is empty");
            for &i in unit {
                assert!(!std::mem::replace(&mut seen[i], true), "function {i} twice");
                out.insert(self.names[i].clone(), u);
            }
        }
        assert!(seen.iter().all(|&s| s), "a function is in no unit");
        out
    }
}

/// How many functions present in both builds compile in another unit.
fn moved(before: &HashMap<String, usize>, after: &HashMap<String, usize>) -> usize {
    before
        .iter()
        .filter(|(name, unit)| after.get(*name).is_some_and(|other| other != *unit))
        .count()
}

const CLASSES: usize = 400;
const UNITS: usize = 8;

#[test]
fn the_switch_is_on_unless_turned_off() {
    assert!(locality_enabled_from_env(None));
    assert!(locality_enabled_from_env(Some("1")));
    for off in ["0", "off", "false"] {
        assert!(!locality_enabled_from_env(Some(off)));
    }
}

#[test]
fn every_function_is_in_exactly_one_of_n_units() {
    let f = Fixture::library(CLASSES);
    let units = units_by_locality(&f.nodes(), &f.edges, UNITS);
    assert_eq!(units.len(), UNITS);
    f.by_name(&units);
    // Unit order is definition order of the units' first functions.
    let firsts: Vec<usize> = units.iter().map(|u| u[0]).collect();
    assert!(firsts.windows(2).all(|w| w[0] < w[1]));
    // Few functions: one unit each, never an empty unit.
    let small = Fixture::library(1);
    let units = units_by_locality(&small.nodes(), &small.edges, 64);
    assert!(units.len() <= small.names.len());
    small.by_name(&units);
    let one = units_by_locality(&f.nodes(), &f.edges, 1);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].len(), f.names.len());
}

#[test]
fn callers_compile_with_their_callees() {
    let f = Fixture::library(CLASSES);
    let unit = f.locality(UNITS);
    for c in 0..CLASSES {
        let m = |k: usize| unit[&format!("perry_method_lib__C{c}__m{k}")];
        // The private helper, and the recursion pair behind it.
        assert_eq!(m(0), m(1), "class {c}: single-caller callee");
        assert_eq!(m(1), m(2), "class {c}: recursion cycle");
        // The closure body with the method that creates it.
        assert_eq!(
            m(4),
            unit[&format!("perry_closure_lib__{c}")],
            "class {c}: closure"
        );
    }
    // The size partition keeps none of that by rule: it separates most pairs.
    let size = f.size(UNITS);
    let split = (0..CLASSES)
        .filter(|c| {
            size[&format!("perry_method_lib__C{c}__m0")]
                != size[&format!("perry_method_lib__C{c}__m1")]
        })
        .count();
    assert!(
        split > CLASSES / 2,
        "size partition split only {split} pairs"
    );
}

#[test]
fn a_registrar_gathers_nothing() {
    // The initializer names every method. Were those references edges of the
    // clustering, the methods defined first would follow it to the last unit.
    let f = Fixture::library(CLASSES);
    let unit = f.locality(UNITS);
    assert_eq!(unit["lib__init_body"], UNITS - 1);
    assert_eq!(unit["perry_method_lib__C0__m5"], 0);
    assert_eq!(unit["perry_method_lib__C0__m0"], 0);
}

#[test]
fn a_cluster_is_cut_at_its_weakest_reference() {
    // a -> b is a call of a single-caller function, c -> a a call of a
    // function with two callers, d names a by address. With room for two
    // functions only, a keeps b.
    let names = ["a", "b", "c", "d", "e"];
    let nodes: Vec<UnitNode<'_>> = names
        .iter()
        .map(|name| UnitNode { name, bytes: 100 })
        .collect();
    let edge = |from, to, direct| UnitEdge { from, to, direct };
    let edges = [
        edge(3, 0, false),
        edge(2, 0, true),
        edge(4, 0, true),
        edge(0, 1, true),
    ];
    let groups = clusters(&nodes, &edges, 200);
    assert_eq!(groups, vec![vec![0, 1], vec![2], vec![3], vec![4]]);
    // With room for three, the caller defined first joins; the address
    // reference stays the weakest.
    let groups = clusters(&nodes, &edges, 300);
    assert_eq!(groups, vec![vec![0, 1, 2], vec![3], vec![4]]);
}

#[test]
fn the_cluster_cap_does_not_follow_small_changes_of_the_total() {
    assert_eq!(cluster_cap(18 << 20), 512 << 10);
    assert_eq!(cluster_cap((18 << 20) + 4096), 512 << 10);
    assert_eq!(cluster_cap(20 << 20), 512 << 10);
    assert_eq!(cluster_cap(1), 1);
    assert!(cluster_cap(18 << 20) * WINDOW_DIV <= 18 << 20);
}

#[test]
fn the_partition_is_deterministic() {
    let f = Fixture::library(CLASSES);
    let first = units_by_locality(&f.nodes(), &f.edges, UNITS);
    assert_eq!(first, units_by_locality(&f.nodes(), &f.edges, UNITS));
    // The order the references were found in, and repeated references, do
    // not matter.
    let mut edges = f.edges.clone();
    edges.reverse();
    edges.extend(f.edges.iter().copied().step_by(3));
    assert_eq!(first, units_by_locality(&f.nodes(), &edges, UNITS));
}

#[test]
fn adding_a_function_moves_almost_nothing() {
    let f = Fixture::library(CLASSES);
    let count = f.names.len();
    for at in [count / 7, count / 2, count - 40] {
        let g = f.with_inserted(at, "perry_fn_lib__addedAndUnused", 4_000);
        let by_locality = moved(&f.locality(UNITS), &g.locality(UNITS));
        let by_size = moved(&f.size(UNITS), &g.size(UNITS));
        assert!(
            by_locality * 100 <= count,
            "insert at {at}: {by_locality} of {count} functions changed unit"
        );
        assert!(
            by_size * 4 >= count,
            "insert at {at}: the size partition moved only {by_size} of {count}"
        );
    }
}

#[test]
fn growing_a_function_moves_almost_nothing() {
    let f = Fixture::library(CLASSES);
    let count = f.names.len();
    for class in [3, CLASSES / 2, CLASSES - 5] {
        let mut g = f.clone();
        let at = g.index(&format!("perry_method_lib__C{class}__m3"));
        g.bytes[at] += 180;
        let by_locality = moved(&f.locality(UNITS), &g.locality(UNITS));
        let by_size = moved(&f.size(UNITS), &g.size(UNITS));
        assert!(
            by_locality * 100 <= count,
            "class {class}: {by_locality} of {count} functions changed unit"
        );
        assert!(
            by_size * 4 >= count,
            "class {class}: the size partition moved only {by_size} of {count}"
        );
    }
}

#[test]
fn renaming_a_function_moves_almost_nothing() {
    let f = Fixture::library(CLASSES);
    let count = f.names.len();
    // m0 is its cluster's anchor (its name is hashed for the cut), m5 is a
    // cluster of its own, util2 is called from eighty classes.
    for name in [
        format!("perry_method_lib__C{}__m0", CLASSES / 2),
        format!("perry_method_lib__C{}__m5", CLASSES / 3),
        "lib__util2".to_string(),
    ] {
        let mut g = f.clone();
        let at = g.index(&name);
        g.names[at] = format!("{name}Renamed");
        let (before, mut after) = (f.locality(UNITS), g.locality(UNITS));
        let unit = after.remove(&format!("{name}Renamed")).unwrap();
        after.insert(name.clone(), unit);
        let by_locality = moved(&before, &after);
        assert!(
            by_locality * 100 <= count,
            "{name}: {by_locality} of {count} functions changed unit"
        );
    }
}

#[test]
fn units_stay_within_the_balance_bound() {
    for (classes, n) in [(CLASSES, UNITS), (CLASSES, 3), (90, 16), (40, 2)] {
        let f = Fixture::library(classes);
        let units = units_by_locality(&f.nodes(), &f.edges, n);
        assert_eq!(units.len(), n);
        let unit_bytes = f.total().div_ceil(n);
        // A cut is at most a window from its balanced position, or half the
        // one cluster that covers the window.
        let slack = (unit_bytes / WINDOW_DIV).max(cluster_cap(unit_bytes));
        for unit in &units {
            let bytes: usize = unit.iter().map(|&i| f.bytes[i]).sum();
            assert!(
                bytes <= unit_bytes + 2 * slack && bytes + 2 * slack >= unit_bytes,
                "{classes} classes in {n} units: a unit of {bytes} bytes, balanced {unit_bytes}"
            );
        }
    }
}

#[test]
fn a_function_larger_than_a_unit_is_a_unit_of_its_own() {
    let mut f = Fixture::library(60);
    let giant = f.index("perry_method_lib__C30__m5");
    f.bytes[giant] = f.total();
    let units = units_by_locality(&f.nodes(), &f.edges, 4);
    assert_eq!(units.len(), 4);
    let home = units.iter().find(|u| u.contains(&giant)).unwrap();
    assert_eq!(home, &vec![giant]);
    // The rest shares the remaining three units evenly.
    let rest: usize = f.total() - f.bytes[giant];
    for unit in units.iter().filter(|u| !u.contains(&giant)) {
        let bytes: usize = unit.iter().map(|&i| f.bytes[i]).sum();
        assert!(bytes * 3 <= rest + rest / 4 && bytes * 3 + rest / 4 >= rest);
    }
}

#[test]
fn the_size_partition_is_the_first_one() {
    // Largest first, each into the smallest unit so far.
    assert_eq!(
        units_by_size(&[10, 70, 30, 20, 60], 2),
        vec![vec![1, 3, 0], vec![4, 2]]
    );
}
