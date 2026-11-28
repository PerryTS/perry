use super::*;
use std::collections::HashMap;

/// Own keys per node, standing in for the object-model glue.
struct World {
    own: HashMap<usize, Vec<OwnKey>>,
}

impl World {
    fn new() -> Self {
        World {
            own: HashMap::new(),
        }
    }
    fn set(&mut self, n: *mut ChainNode, keys: &[(u64, u64, u32)]) {
        self.own.insert(
            n as usize,
            keys.iter()
                .map(|&(key_bits, hash, loc)| OwnKey {
                    load: 0,
                    key_bits,
                    hash,
                    loc,
                    act: act::DATA_INLINE,
                    enumerable: true,
                })
                .collect(),
        );
    }
    unsafe fn rebuild(&self, root: *mut ChainNode) -> (usize, usize) {
        rebuild_subtree(root, |n, out| {
            if let Some(k) = self.own.get(&(n as usize)) {
                out.extend_from_slice(k);
            }
        })
    }
}

unsafe fn holder(d: *const ChainDesc, key: u64, hash: u64) -> Option<*mut ChainNode> {
    match ChainDesc::lookup(d, key, hash) {
        Found::Hit(e) => Some((*e).holder),
        _ => None,
    }
}

#[test]
fn own_key_shadows_parent() {
    unsafe {
        let mut w = World::new();
        let p = new_node(1, false);
        let c = new_node(2, false);
        link(p, ptr::null_mut());
        link(c, p);
        w.set(p, &[(100, 7, 0), (101, 8, 1)]);
        w.set(c, &[(100, 7, 3)]);
        w.rebuild(p);
        assert_eq!(holder((*c).desc, 100, 7), Some(c));
        assert_eq!(holder((*c).desc, 101, 8), Some(p));
        assert!(matches!(ChainDesc::lookup((*c).desc, 102, 9), Found::Miss(b) if b.is_null()));
        free_node(c);
        free_node(p);
    }
}

#[test]
fn delete_on_ancestor_reveals_grandparent_after_subtree_rebuild() {
    unsafe {
        let mut w = World::new();
        let g = new_node(1, false);
        let p = new_node(2, false);
        let c = new_node(3, false);
        link(g, ptr::null_mut());
        link(p, g);
        link(c, p);
        w.set(g, &[(100, 5, 0)]);
        w.set(p, &[(100, 5, 1)]);
        w.rebuild(g);
        assert_eq!(holder((*c).desc, 100, 5), Some(p));
        w.set(p, &[]);
        let (nodes, _) = w.rebuild(p);
        assert_eq!(nodes, 2);
        assert_eq!(holder((*c).desc, 100, 5), Some(g));
        for n in [c, p, g] {
            free_node(n);
        }
    }
}

#[test]
fn pre_order_rebuilds_ancestor_before_descendant_depth_3_with_siblings() {
    unsafe {
        let mut w = World::new();
        let root = new_node(1, false);
        link(root, ptr::null_mut());
        let a = new_node(2, false);
        let b = new_node(3, false);
        link(a, root);
        link(b, root);
        let a1 = new_node(4, false);
        link(a1, a);
        let a11 = new_node(5, false);
        link(a11, a1);
        w.set(root, &[(200, 1, 0)]);
        let mut order = Vec::new();
        rebuild_subtree(root, |n, out| {
            // Every parent's desc is already the fresh one when a child builds.
            if !(*n).parent.is_null() {
                assert!(order.contains(&((*n).parent as usize)));
            }
            order.push(n as usize);
            if let Some(k) = w.own.get(&(n as usize)) {
                out.extend_from_slice(k);
            }
        });
        assert_eq!(order.len(), 5);
        w.set(root, &[(201, 2, 0)]);
        w.rebuild(root);
        assert_eq!(holder((*a11).desc, 201, 2), Some(root));
        assert_eq!(holder((*a11).desc, 200, 1), None);
        assert_eq!(holder((*b).desc, 201, 2), Some(root));
        for n in [a11, a1, a, b, root] {
            free_node(n);
        }
    }
}

#[test]
fn equal_hash_different_key_bits_never_match() {
    unsafe {
        let mut w = World::new();
        let p = new_node(1, false);
        link(p, ptr::null_mut());
        w.set(p, &[(300, 42, 0), (301, 42, 1), (302, 42, 2)]);
        w.rebuild(p);
        assert_eq!(holder((*p).desc, 301, 42), Some(p));
        match ChainDesc::lookup((*p).desc, 303, 42) {
            Found::Miss(b) => assert!(b.is_null()),
            other => panic!("colliding hash matched: {other:?}"),
        }
        free_node(p);
    }
}

#[test]
fn exotic_node_is_boundary_and_keeps_entries_below_it() {
    unsafe {
        let mut w = World::new();
        let x = new_node(1, false);
        let proxy = new_node(2, true);
        let p = new_node(3, false);
        link(x, ptr::null_mut());
        link(proxy, x);
        link(p, proxy);
        w.set(x, &[(400, 1, 0)]);
        w.set(proxy, &[(401, 2, 0)]);
        w.set(p, &[(402, 3, 0)]);
        w.rebuild(x);
        assert_eq!(holder((*p).desc, 402, 3), Some(p));
        // Keys of and above the exotic node do not answer: the real [[Get]] does.
        assert!(matches!(ChainDesc::lookup((*p).desc, 400, 1), Found::Miss(b) if b == proxy));
        assert!(matches!(ChainDesc::lookup((*p).desc, 401, 2), Found::Miss(b) if b == proxy));
        for n in [p, proxy, x] {
            free_node(n);
        }
    }
}

#[test]
fn text_entry_needs_exact_compare() {
    unsafe {
        let p = new_node(1, false);
        link(p, ptr::null_mut());
        let own = [OwnKey {
            load: 0,
            key_bits: 500,
            hash: 9,
            loc: 0,
            act: act::TEXT,
            enumerable: true,
        }];
        (*p).desc = build(p, &own);
        assert!(matches!(
            ChainDesc::lookup((*p).desc, 777, 9),
            Found::Text(_)
        ));
        assert!(matches!(
            ChainDesc::lookup((*p).desc, 777, 10),
            Found::Miss(_)
        ));
        free_node(p);
    }
}

#[test]
fn probe_chains_stay_bounded_under_clustered_hashes() {
    unsafe {
        let p = new_node(1, false);
        link(p, ptr::null_mut());
        let own: Vec<OwnKey> = (0..64u64)
            .map(|i| OwnKey {
                load: 0,
                key_bits: 1000 + i,
                hash: (i % 3) << 20,
                loc: i as u32,
                act: act::DATA_INLINE,
                enumerable: true,
            })
            .collect();
        // Three distinct full hashes for 64 keys: growth cannot separate
        // them; the build stops at its cap and every key is still exact.
        (*p).desc = build(p, &own);
        assert_eq!((*(*p).desc).count, 64);
        for k in &own {
            assert_eq!(holder((*p).desc, k.key_bits, k.hash), Some(p));
        }
        free_node(p);
    }
}

#[test]
fn relink_moves_subtree() {
    unsafe {
        let mut w = World::new();
        let p = new_node(1, false);
        let q = new_node(2, false);
        let c = new_node(3, false);
        link(p, ptr::null_mut());
        link(q, ptr::null_mut());
        link(c, p);
        w.set(p, &[(600, 1, 0)]);
        w.set(q, &[(601, 2, 0)]);
        w.rebuild(p);
        w.rebuild(q);
        assert_eq!(holder((*c).desc, 600, 1), Some(p));
        unlink(c);
        link(c, q);
        w.rebuild(c);
        assert_eq!(holder((*c).desc, 600, 1), None);
        assert_eq!(holder((*c).desc, 601, 2), Some(q));
        assert!((*p).first_child.is_null());
        for n in [c, p, q] {
            free_node(n);
        }
    }
}
