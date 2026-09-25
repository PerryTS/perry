//! One backing per growth chain: the lists `[a]`, `[a,b]`, ... share one
//! array, a node is `(backing, count)`, and the shape owns the count.

use super::*;
use crate::object::ObjectKeys;

fn key(name: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// The names a view exposes, read through its own count — never the
/// backing's header length.
unsafe fn names_of(view: ObjectKeys) -> Vec<String> {
    let (slots, len) = view.dense_slots();
    assert_eq!(
        len,
        view.count() as usize,
        "a view must expose exactly its count"
    );
    (0..len)
        .map(|i| {
            let value = JSValue::from_bits((*slots.add(i)).to_bits());
            let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
            match crate::string::js_string_key_bytes(value, &mut sso) {
                Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                None => "<hole>".to_string(),
            }
        })
        .collect()
}

unsafe fn chain(proof: &SharedLayout, names: &[&str]) -> Vec<CanonicalKeys> {
    let mut out = Vec::new();
    let mut cur = CanonicalKeys::EMPTY;
    for name in names {
        cur = extend_key(proof, cur, key(name));
        out.push(cur);
    }
    out
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// A plain object whose keys were added one at a time, in `names` order.
unsafe fn object_with(names: &[&str]) -> *mut crate::object::ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, names.len() as u32));
    for (i, name) in names.iter().enumerate() {
        let k = key(name);
        obj.with_mut_ptr(|o| crate::object::js_object_set_field_by_name(o, k, i as f64));
    }
    obj.with_mut_ptr(|o| o)
}

unsafe fn object_names(obj: crate::gc::RuntimeHandle<'_>) -> Vec<String> {
    obj.with_const_ptr(|o| names_of(crate::object::object_keys(o)))
}

/// The chain `[a] -> [a,b] -> [a,b,c] -> [a,b,c,d]` is ONE array: every
/// extension of the tip appends in place, and each list reads only its own
/// prefix of it.
#[test]
fn a_growth_chain_shares_one_backing() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let lists = chain(&proof, &["a", "b", "c", "d"]);
        let backing = lists[0].as_ptr();
        for (i, list) in lists.iter().enumerate() {
            assert_eq!(
                list.as_ptr(),
                backing,
                "list {} left the chain's backing",
                i + 1
            );
            assert_eq!(list.len(), i as u32 + 1);
        }
        assert_eq!(
            (*backing).length,
            4,
            "the backing's length is its tip's count"
        );
        assert_eq!(names_of(lists[1].view()), strings(&["a", "b"]));
        assert_eq!(names_of(lists[3].view()), strings(&["a", "b", "c", "d"]));
        // The whole-list funnel and the grow path name the same node.
        let mut raw = crate::array::js_array_alloc(2);
        for name in ["a", "b"] {
            raw = crate::array::js_array_push(raw, JSValue::string_ptr(key(name)));
        }
        assert_eq!(canonicalize(&proof, raw, 2), lists[1]);
        // Re-extending an interior list is a hit, not a fork.
        assert_eq!(extend_key(&proof, lists[1], key("c")), lists[2]);
        assert_eq!((*backing).length, 4);
    }
}

/// A different key appended to a list the backing has grown past forks: the
/// prefix is copied into a new backing, and neither the old backing nor any
/// list on it changes. A full backing continues on a new one the same way.
#[test]
fn a_fork_and_a_full_backing_start_a_new_backing() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let lists = chain(&proof, &["a", "b", "c", "d"]);
        let backing = lists[0].as_ptr();
        let fork = extend_key(&proof, lists[1], key("x"));
        assert_ne!(
            fork.as_ptr(),
            backing,
            "a fork must not write the shared backing"
        );
        assert_eq!(names_of(fork.view()), strings(&["a", "b", "x"]));
        assert_eq!((*backing).length, 4);
        assert_eq!(names_of(lists[1].view()), strings(&["a", "b"]));
        assert_eq!(names_of(lists[3].view()), strings(&["a", "b", "c", "d"]));
        // The fork is the tip of ITS backing and keeps growing in place.
        let fork_tip = extend_key(&proof, fork, key("y"));
        assert_eq!(fork_tip.as_ptr(), fork.as_ptr());
        assert_eq!(names_of(fork_tip.view()), strings(&["a", "b", "x", "y"]));
        // [a,b,c,d] filled its 4-slot backing: the fifth key continues on a
        // new, larger backing and the old one is untouched.
        let fifth = extend_key(&proof, lists[3], key("e"));
        assert_ne!(fifth.as_ptr(), backing);
        assert!(
            (*fifth.as_ptr()).capacity > 5,
            "a new backing must leave room to grow"
        );
        assert_eq!(names_of(fifth.view()), strings(&["a", "b", "c", "d", "e"]));
        assert_eq!((*backing).length, 4);
        // A tombstone is not a heap pointer: an all-pointer backing forks
        // rather than take it, so its collector layout stays true.
        let hole = extend_slot(
            &proof,
            fifth,
            Appended::Slot(JSValue::from_bits(crate::value::TAG_HOLE)),
            0,
        );
        assert_ne!(hole.as_ptr(), fifth.as_ptr());
        assert_eq!(
            names_of(hole.view()),
            strings(&["a", "b", "c", "d", "e", "<hole>"])
        );
        assert_eq!(names_of(fifth.view()), strings(&["a", "b", "c", "d", "e"]));
    }
}

/// The owner's writer-isolation test: a live `[a,b]` and an extended
/// `[a,b,c]` tip on ONE backing with room to grow; every key writer runs on
/// an object holding each of them, and the other lists' views never change.
/// On the tip, an append is the one in-place write a backing ever takes.
#[test]
fn every_writer_leaves_the_other_lists_on_a_backing_unchanged() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        for tombstones in [false, true] {
            let _mode = crate::object::delete_rest::test_scope_tombstone_deletes(tombstones);
            for (writer, name) in WRITERS.iter().enumerate() {
                for mutate_tip in [false, true] {
                    reset_for_test();
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let tip = scope.root_raw_mut_ptr(object_with(&["a", "b", "c"]));
                    let short = scope.root_raw_mut_ptr(object_with(&["a", "b"]));
                    let target = scope.root_raw_mut_ptr(if mutate_tip {
                        object_with(&["a", "b", "c"])
                    } else {
                        object_with(&["a", "b"])
                    });
                    // Premise: the objects' lists really do share a backing.
                    let (tip_keys, short_keys, target_keys) = (
                        tip.with_const_ptr(|o| crate::object::object_keys(o)),
                        short.with_const_ptr(|o| crate::object::object_keys(o)),
                        target.with_const_ptr(|o| crate::object::object_keys(o)),
                    );
                    assert_eq!(tip_keys.arr(), short_keys.arr(), "premise: one backing");
                    assert_eq!(target_keys.arr(), tip_keys.arr(), "premise: one backing");
                    assert_eq!((tip_keys.count(), short_keys.count()), (3, 2));
                    assert!(
                        (*tip_keys.arr()).capacity > 3,
                        "premise: the tip can grow in place"
                    );

                    run_writer(writer, target);

                    let label = format!("writer {name} (tip={mutate_tip} tomb={tombstones})");
                    assert_eq!(object_names(tip), strings(&["a", "b", "c"]), "{label}");
                    assert_eq!(object_names(short), strings(&["a", "b"]), "{label}");
                    let short_keys = short.with_const_ptr(|o| crate::object::object_keys(o));
                    let tip_keys = tip.with_const_ptr(|o| crate::object::object_keys(o));
                    assert_eq!(short_keys.arr(), tip_keys.arr(), "{label}: shape moved");
                }
            }
        }
    }
}

const WRITERS: [&str; 8] = [
    "append-new",
    "append-next-on-chain",
    "delete-first",
    "delete-last",
    "define-property",
    "dictionary-then-append",
    "keys-result-mutation",
    "delete-then-readd",
];

unsafe fn run_writer(writer: usize, obj: crate::gc::RuntimeHandle<'_>) {
    let set = |name: &str, value: f64| {
        let k = key(name);
        obj.with_mut_ptr(|o| crate::object::js_object_set_field_by_name(o, k, value));
    };
    let delete = |name: &str| {
        let k = key(name);
        assert_eq!(
            obj.with_mut_ptr(|o| crate::object::js_object_delete_field(o, k)),
            1
        );
    };
    match writer {
        0 => set("z", 1.0),
        1 => {
            set("c", 1.0);
            set("q", 2.0);
        }
        2 => delete("a"),
        3 => delete("b"),
        4 => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            let k = key("value");
            descriptor.with_mut_ptr(|d| crate::object::js_object_set_field_by_name(d, k, 17.0));
            let defined = key("defined");
            obj.with_const_ptr::<crate::ObjectHeader, _>(|o| {
                descriptor.with_const_ptr::<crate::ObjectHeader, _>(|d| {
                    crate::object::js_object_define_property(
                        crate::value::js_nanbox_pointer(o as i64),
                        crate::value::js_nanbox_string(defined as i64),
                        crate::value::js_nanbox_pointer(d as i64),
                    );
                })
            });
        }
        5 => {
            assert!(obj.with_mut_ptr(|o| crate::object::dictionary::latch_object_to_dictionary(o)));
            set("z", 1.0);
            delete("a");
        }
        6 => {
            let result = obj.with_const_ptr(|o| crate::object::js_object_keys(o));
            let result = crate::array::js_array_sort_default(result);
            crate::array::js_array_set(result, 0, JSValue::number(99.0));
            let result = crate::array::js_array_push(result, JSValue::number(100.0));
            crate::array::js_array_set_length(result, 0.0);
        }
        7 => {
            delete("a");
            set("a", 3.0);
            set("b", 4.0);
        }
        _ => unreachable!(),
    }
}

/// One slot index serves every list on a backing, and a shorter list must
/// not see a key past its count: the index covers `[k0..k39]`, and the
/// 35-key list on the same backing must answer ABSENT for `k38`, which the
/// linear-scan backstop would never re-check. (35 and 40 share the backing
/// the chain started at 31 keys; an earlier length sits on an earlier one.)
#[test]
fn a_shorter_list_on_an_indexed_backing_is_absent_past_its_count() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let names: Vec<String> = (0..40).map(|i| format!("k{i}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let scope = crate::gc::RuntimeHandleScope::new();
        let long = scope.root_raw_mut_ptr(object_with(&names));
        let short = scope.root_raw_mut_ptr(object_with(&names[..35]));
        let (long_keys, short_keys) = (
            long.with_const_ptr(|o| crate::object::object_keys(o)),
            short.with_const_ptr(|o| crate::object::object_keys(o)),
        );
        assert_eq!(long_keys.arr(), short_keys.arr(), "premise: one backing");
        let verdict = |view: ObjectKeys, name: &str| {
            let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
            crate::object::shapes::shape_slot_lookup_verdict(
                view.arr(),
                name.as_bytes(),
                hash,
                view.count(),
                true,
            )
        };
        use crate::object::shapes::KeysIndexVerdict::{Absent, Found};
        // Index the whole backing through the long list first.
        assert!(matches!(verdict(long_keys, "k38"), Found(38)));
        assert!(matches!(verdict(short_keys, "k33"), Found(33)));
        assert!(
            matches!(verdict(short_keys, "k38"), Absent),
            "a 35-key list found a key at slot 38 of its shared backing"
        );
        // And the same through the object model.
        let k38 = key("k38");
        let got = short.with_const_ptr(|o| crate::object::js_object_get_field_by_name(o, k38));
        assert!(
            got.is_undefined(),
            "the short object must not read the tip's k38"
        );
        let got = long.with_const_ptr(|o| crate::object::js_object_get_field_by_name(o, k38));
        assert_eq!(got.as_number(), 38.0);
    }
}

/// Building an N-key object one key at a time indexes O(N) slots in total.
/// When every length was its own array, each find-before-append indexed the
/// whole list from nothing: N^2/2 slots (2,199,657 on ts.transpileModule,
/// against main's 75,775).
#[test]
fn building_a_wide_object_indexes_linear_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        const N: usize = 400;
        let before = crate::object::shapes::indexed_slots_for_test();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for i in 0..N {
            let k = key(&format!("wide_key_{i}"));
            obj.with_mut_ptr(|o| crate::object::js_object_set_field_by_name(o, k, i as f64));
        }
        assert_eq!(object_names(obj).len(), N);
        let indexed = crate::object::shapes::indexed_slots_for_test() - before;
        eprintln!("wide object: {N} keys, {indexed} indexed slots");
        assert!(
            indexed > 0,
            "premise: the write path must have built an index"
        );
        assert!(
            indexed <= 4 * N as u64,
            "{indexed} indexed slots for a {N}-key object; expected O(N)"
        );
    }
}
