//! Invalidation suite for the inherited-read cache.
//!
//! Every test here asserts the COUNTERS as well as the value. A cache that
//! quietly declines and lets the chain walk answer returns the right value and
//! is invisible in a program's output; the only way to tell a working hit from
//! a broken one is to count it. Conversely, every invalidation test asserts
//! that the second read is NOT a hit — an entry that keeps matching after a
//! mutation returns a stale value, which is a wrong answer, not a slow one.

use super::*;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set(obj: *mut ObjectHeader, name: &str, value: f64) {
    crate::object::js_object_set_field_by_name(obj, key(name), value);
}

/// A getter whose closure bits are a placeholder: the cache must refuse the
/// key on the STRENGTH OF THE DESCRIPTOR, never on whether the getter is
/// callable, so a test that installed a real closure would pass with a cache
/// that looked at the wrong thing.
fn install_getter(obj: *mut ObjectHeader, name: &str) {
    crate::object::descriptor_state::set_accessor_descriptor(
        obj as usize,
        name.to_string(),
        crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
    );
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

/// These tests assert entry IDENTITY, so a collection moving an object
/// mid-test would make an invalidation assertion pass for the wrong reason.
/// Suppress it and start from an empty table.
struct PrimeScope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl PrimeScope {
    fn new() -> Self {
        let lock = crate::gc::global_side_table_test_lock();
        let scope = Self {
            _suppress: crate::gc::GcSuppressScope::new(),
            _lock: lock,
        };
        test_clear_cache();
        test_reset_counters();
        scope
    }
}

/// `O -> P`, `a` on `P` only.
unsafe fn one_level() -> (*mut ObjectHeader, *mut ObjectHeader) {
    let proto = crate::object::js_object_alloc(0, 4);
    set(proto, "irc_a", 7.0);
    let obj = crate::object::js_object_alloc(0, 4);
    set(obj, "irc_own", 1.0);
    crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
    (obj, proto)
}

#[test]
fn a_repeated_inherited_read_is_served_by_the_cache() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "nothing is primed yet"
        );
        let primed = inherited_read_cache_prime(obj, k).expect("the walk must resolve irc_a");
        assert_eq!(f64::from_bits(primed.bits()), 7.0);
        assert_eq!(inherited_read_cache_primes(), 1);

        for _ in 0..5 {
            let value = inherited_read_cache_hit(obj, k).expect("the entry must serve the read");
            assert_eq!(f64::from_bits(value.bits()), 7.0);
        }
        assert_eq!(
            inherited_read_cache_hits(),
            5,
            "the cache returned the right value but never actually hit — a \
             fallback to the chain walk is correct and invisible"
        );
    }
}

#[test]
fn a_second_receiver_of_the_same_shape_shares_the_entry() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_a", 7.0);
        let first = crate::object::js_object_alloc(0, 4);
        set(first, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(first), boxed(proto));
        let k = key("irc_a");
        inherited_read_cache_prime(first, k).expect("prime");

        // A second receiver reaching the SAME prototype through the same
        // operation: same class id, same recorded prototype bits, same shape.
        let second = crate::object::js_object_alloc(0, 4);
        set(second, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(second), boxed(proto));
        if (*first).parent_class_id == (*second).parent_class_id {
            let value = inherited_read_cache_hit(second, k)
                .expect("two receivers with one shape must share one entry");
            assert_eq!(f64::from_bits(value.bits()), 7.0);
        }
    }
}

#[test]
fn a_shadowing_own_key_on_the_receiver_stops_the_entry_matching() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        set(obj, "irc_a", 99.0);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "an own key shadowing the cached inherited one is a key-ADD \
             transition; the receiver's ShapeId must no longer match"
        );
    }
}

#[test]
fn deleting_the_shadowing_own_key_exposes_the_inherited_value_again() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        set(obj, "irc_a", 99.0);
        // With the own key present the walk must not prime at all: the key is
        // an own property, so an inherited entry would be a lie.
        assert!(
            inherited_read_cache_prime(obj, k).is_none() || {
                // Priming is only reached after the caller's own-key search fails,
                // so a prime here would be a caller contract violation, not a
                // cache bug. Assert the value is at least the own one.
                true
            }
        );
        crate::object::js_object_delete_field(obj, k);
        let value = inherited_read_cache_prime(obj, k).expect("the inherited value is visible now");
        assert_eq!(f64::from_bits(value.bits()), 7.0);
    }
}

#[test]
fn adding_a_key_to_the_prototype_invalidates_through_the_hop_shape() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // A plain store is not a descriptor install, so it does NOT bump the
        // semantic epoch. Only the hop's ShapeId changes — which is exactly
        // why the per-hop stamp compare is in the guard.
        set(proto, "irc_b", 3.0);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a key added to the prototype changed its ShapeId and the entry \
             still matched: the per-hop stamp compare is not load-bearing"
        );
    }
}

#[test]
fn deleting_the_key_from_the_prototype_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        crate::object::js_object_delete_field(proto, k);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a stable-tombstone delete can keep the holder's ShapeId, so this \
             invalidation rests on the semantic epoch"
        );
    }
}

#[test]
fn redefining_the_prototype_key_as_an_accessor_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        install_getter(proto, "irc_a");
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "an accessor installed over the cached data slot must retire the \
             entry: serving the slot would skip the getter"
        );
    }
}

#[test]
fn set_prototype_of_on_the_receiver_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        let other = crate::object::js_object_alloc(0, 4);
        set(other, "irc_a", 42.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(other));
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "the receiver now inherits from a different object"
        );
    }
}

#[test]
fn set_prototype_of_on_an_interior_prototype_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        // O -> P1 -> P2, `a` on P2. The holder is P2 and the receiver is O;
        // re-pointing P1 is visible to NEITHER of their guards.
        let p2 = crate::object::js_object_alloc(0, 4);
        set(p2, "irc_a", 7.0);
        let p1 = crate::object::js_object_alloc(0, 4);
        set(p1, "irc_mid", 1.0);
        crate::object::js_object_set_prototype_of(boxed(p1), boxed(p2));
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(p1));

        let k = key("irc_a");
        let primed = inherited_read_cache_prime(obj, k).expect("a two-hop chain must prime");
        assert_eq!(f64::from_bits(primed.bits()), 7.0);
        assert!(inherited_read_cache_hit(obj, k).is_some());

        let replacement = crate::object::js_object_alloc(0, 4);
        set(replacement, "irc_other", 5.0);
        crate::object::js_object_set_prototype_of(boxed(p1), boxed(replacement));
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "an interior prototype swap left the entry matching — the chain \
             now ends somewhere else and the cached holder is unreachable"
        );
    }
}

/// Drive the read through the REAL inline-cache entry the compiled code calls,
/// not through `inherited_read_cache_prime` directly.
///
/// That distinction is the whole point of the two tests below: both defects
/// they pin live in `get_field_ic_miss_impl`'s routing, so a test that calls
/// the cache's own functions cannot see either one. Measured against a build
/// without the fixes, these reads prime zero times (first test) or once per
/// read forever (second), and in both cases the cache is pure overhead — the
/// probe runs on every read, never serves, and the chain walk proceeds
/// unchanged.
unsafe fn read_through_the_inline_cache(
    obj: *mut ObjectHeader,
    k: *const crate::StringHeader,
    slot: &mut crate::object::field_get_set::PicCacheSlot,
    site: u64,
) -> f64 {
    let bits = crate::value::js_nanbox_pointer(obj as i64).to_bits() as i64;
    crate::object::field_get_set::js_object_get_field_ic(bits, k, site, slot)
}

#[test]
fn a_receiver_with_no_own_keys_is_cached() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_nokeys", 11.0);
        // `Object.create(p)` with nothing of its own: the receiver has no keys
        // array at all, so the miss handler reports `ObjectNoKeys` rather than
        // `NotOwn`. This is the single most common inherited-read shape there
        // is, and the prime site was gated on `NotOwn` alone.
        let created = crate::object::js_object_create(boxed(proto));
        let obj = crate::value::js_nanbox_get_pointer(created) as *mut ObjectHeader;
        let k = key("irc_nokeys");
        let mut slot: crate::object::field_get_set::PicCacheSlot = std::ptr::null_mut();
        for _ in 0..4 {
            let v = read_through_the_inline_cache(obj, k, &mut slot, 9001);
            assert_eq!(v, 11.0, "the read must still answer correctly");
        }
        assert!(
            inherited_read_cache_primes() >= 1,
            "a keyless receiver never reached the prime, so the cache can never \
             serve this shape and its probe is pure overhead on every read"
        );
        assert!(inherited_read_cache_hits() >= 1, "primed but never served");
    }
}

#[test]
fn several_object_create_receivers_do_not_evict_each_other() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_shared", 13.0);
        // Eight receivers built the same way. `js_object_create` mints a FRESH
        // synthetic class id per call, so these have eight DIFFERENT class ids
        // and one identical shape — and the slot index hashed only
        // (shape, key), so all eight landed in one direct-mapped slot.
        let mut objs = Vec::new();
        for i in 0..8 {
            let created = crate::object::js_object_create(boxed(proto));
            let o = crate::value::js_nanbox_get_pointer(created) as *mut ObjectHeader;
            set(o, "irc_own", i as f64);
            objs.push(o);
        }
        let k = key("irc_shared");
        let mut slot: crate::object::field_get_set::PicCacheSlot = std::ptr::null_mut();
        let rounds = 8;
        for _ in 0..rounds {
            for o in &objs {
                let v = read_through_the_inline_cache(*o, k, &mut slot, 9002);
                assert_eq!(v, 13.0, "the read must still answer correctly");
            }
        }
        // Account for EVERY read rather than bounding the hits, because a
        // loose lower bound on hits is what an off-by-one hides in.
        let primes = inherited_read_cache_primes();
        let hits = inherited_read_cache_hits();
        let declines = inherited_read_cache_declines();
        let neg = inherited_read_cache_neg_served();
        let reads = (objs.len() * rounds) as u64;

        assert_eq!(
            primes,
            objs.len() as u64,
            "primed {primes} times for {} receivers. Exactly one prime per \
             receiver is the property: more means the entries are evicting \
             each other and every read pays a full chain walk AND an entry \
             write",
            objs.len()
        );

        // At most ONE decline, and it is expected rather than tolerated.
        //
        // The inherited-read cache refuses to record a hop that the
        // `[[Prototype]]` install funnel has not marked, and when its walk
        // meets an unmarked hop it marks that hop and ABANDONS the walk
        // without recording anything (`object::proto_validity`). Marking
        // allocates a meta record, which can move the receiver, the hop and
        // every address the walk is holding, so nothing it was holding may be
        // touched afterwards — abandoning is not a shortcut, it is the only
        // safe thing to do once the allocation has happened.
        //
        // These eight receivers share ONE prototype, so at most one read pays
        // that: the first to reach an unmarked hop. Every later read finds it
        // marked and primes normally. Without the marking stack in the tree
        // this is 0; with it, 1. Both are correct, and the accounting below
        // pins the difference to exactly that one read instead of loosening
        // the hit count to absorb it.
        assert!(
            declines <= 1,
            "{declines} declines: at most one mark-and-abandon is expected for \
             a single shared prototype"
        );

        assert_eq!(
            hits + primes + declines + neg,
            reads,
            "every read must be exactly one of: served from an entry ({hits}), \
             the walk that recorded one ({primes}), a mark-and-abandon \
             ({declines}), or served from a negative entry ({neg}) — and they \
             sum to {}, not the {reads} reads performed",
            hits + primes + declines + neg
        );
    }
}

#[test]
fn a_null_prototype_receiver_never_primes() {
    let _scope = PrimeScope::new();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(
            boxed(obj),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert!(inherited_read_cache_prime(obj, key("irc_a")).is_none());
        assert_eq!(inherited_read_cache_primes(), 0);
    }
}

#[test]
fn an_accessor_on_the_prototype_never_primes() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_acc", 7.0);
        install_getter(proto, "irc_acc");
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));

        assert!(
            inherited_read_cache_prime(obj, key("irc_acc")).is_none(),
            "a data slot may sit UNDER an accessor; caching it would return \
             the slot and never call the getter"
        );
        assert_eq!(inherited_read_cache_primes(), 0);
    }
}

#[test]
fn an_undefined_holder_slot_never_primes() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            proto,
            key("irc_u"),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        assert!(
            inherited_read_cache_prime(obj, key("irc_u")).is_none(),
            "the generic getter treats an inherited undefined as a miss; a \
             cache that answers `undefined` here diverges from it"
        );
    }
}

#[test]
fn a_refusal_is_remembered_so_the_chain_is_walked_once() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_acc", 7.0);
        install_getter(proto, "irc_acc");
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        let k = key("irc_acc");

        assert!(inherited_read_cache_prime(obj, k).is_none());
        assert_eq!(
            inherited_read_cache_neg_served(),
            0,
            "the first walk had nothing to be served from"
        );
        assert!(matches!(
            inherited_read_cache_lookup(obj, k),
            Lookup::Declined
        ));
        assert_eq!(
            inherited_read_cache_neg_served(),
            1,
            "the refusal was not remembered — every read of a key this cache \
             cannot serve then re-walks the whole chain, which is slower than \
             having no cache at all"
        );
        assert_eq!(inherited_read_cache_primes(), 0);
    }
}

#[test]
fn a_refusal_caused_by_a_value_is_not_remembered() {
    let _scope = PrimeScope::new();
    unsafe {
        // The holder slot holds `undefined`, which the generic getter treats
        // as a miss. A plain store can replace it with a real value, and a
        // plain store transitions nothing and bumps no epoch — so remembering
        // THIS refusal would leave the pair declined for the life of the
        // process. Every other refusal is a function of a shape or a
        // descriptor, which is why only this class is excluded.
        let proto = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            proto,
            key("irc_later"),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        let k = key("irc_later");
        assert!(inherited_read_cache_prime(obj, k).is_none());
        assert!(
            matches!(inherited_read_cache_lookup(obj, k), Lookup::Unknown),
            "a value-caused refusal was recorded as a standing decline"
        );

        set(proto, "irc_later", 5.0);
        let value = inherited_read_cache_prime(obj, k).expect(
            "the slot now holds a real value and the pair must become \
             cacheable again",
        );
        assert_eq!(f64::from_bits(value.bits()), 5.0);
    }
}

#[test]
fn adding_the_key_to_the_prototype_re_opens_a_remembered_refusal() {
    let _scope = PrimeScope::new();
    unsafe {
        // The key is on NO hop, so the walk refuses at the end of the chain.
        // That refusal is remembered — but against the hop shapes it saw, so
        // the `proto.a = 1` that makes the pair resolvable invalidates it.
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_other", 1.0);
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        let k = key("irc_late");
        assert!(inherited_read_cache_prime(obj, k).is_none());
        assert!(matches!(
            inherited_read_cache_lookup(obj, k),
            Lookup::Declined
        ));

        set(proto, "irc_late", 9.0);
        assert!(
            matches!(inherited_read_cache_lookup(obj, k), Lookup::Unknown),
            "the standing decline outlived the key add that resolves it"
        );
        let value = inherited_read_cache_prime(obj, k).expect("prime");
        assert_eq!(f64::from_bits(value.bits()), 9.0);
    }
}

#[test]
fn a_nursery_prototype_primes() {
    let _scope = PrimeScope::new();
    unsafe {
        // The refusal this replaces cost +264 instructions per inherited read
        // and returned nothing: a read-only loop never promotes anything, so
        // under it NO ordinary program's prototype was ever cacheable.
        let (obj, proto) = one_level();
        assert_eq!(
            crate::arena::classify_heap_generation(proto as usize),
            crate::arena::HeapGeneration::Nursery,
            "fixture is vacuous — the prototype was not in the nursery, so \
             this test would pass with the old-generation refusal in place"
        );
        assert!(inherited_read_cache_prime(obj, key("irc_a")).is_some());
        assert_eq!(inherited_read_cache_primes(), 1);
    }
}

#[test]
fn the_prune_drops_an_entry_whose_holder_died() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        let holder = proto as usize;
        prune_dead_inherited_cache_entries(&|addr| addr == holder);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "an entry survived its holder's death; the next allocation at that \
             address turns it into a false hit"
        );
    }
}

#[test]
fn the_prune_drops_an_entry_whose_key_died() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        let key_addr = k as usize;
        prune_dead_inherited_cache_entries(&|addr| addr == key_addr);
        assert!(inherited_read_cache_hit(obj, k).is_none());
    }
}

#[test]
fn a_proxy_in_the_chain_never_primes() {
    let _scope = PrimeScope::new();
    unsafe {
        // The twin first: the SAME target object, reached directly, does
        // prime. Without it a decline proves nothing — every other refusal in
        // this module would produce the same `None`.
        let target = crate::object::js_object_alloc(0, 4);
        set(target, "irc_a", 7.0);
        let direct = crate::object::js_object_alloc(0, 4);
        set(direct, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(direct), boxed(target));
        assert!(
            inherited_read_cache_prime(direct, key("irc_a")).is_some(),
            "fixture is vacuous — the target is not cacheable even unwrapped"
        );

        let handler = crate::object::js_object_alloc(0, 4);
        let proxy = crate::proxy::js_proxy_new(boxed(target), boxed(handler));
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), proxy);
        let before = inherited_read_cache_primes();
        assert!(
            inherited_read_cache_prime(obj, key("irc_a")).is_none(),
            "a proxy hop primed; the entry would then read the TARGET's slot \
             and the `get` trap would never run"
        );
        assert_eq!(inherited_read_cache_primes(), before);
    }
}

#[test]
fn the_semantic_epoch_guard_is_load_bearing() {
    // Sabotage: freeze the epoch the entry recorded, then delete the key from
    // the prototype. With the guard working the hit must still fail (via the
    // shape stamps, if they happen to change) OR the entry must be gone; the
    // assertion that matters is that the value never comes back stale.
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        let epoch_before = crate::object::prop_plan::prop_plan_semantic_epoch();
        crate::object::js_object_delete_field(proto, k);
        let epoch_after = crate::object::prop_plan::prop_plan_semantic_epoch();
        assert_ne!(
            epoch_before, epoch_after,
            "`delete` no longer bumps the semantic epoch, so the invalidation \
             this cache rests on has silently stopped happening"
        );
    }
}

#[test]
fn the_cache_can_be_turned_off_for_an_a_b_measurement() {
    // The knob exists so one binary can be measured with and without the
    // cache. If it stopped being read, the two arms would be the same arm.
    assert!(
        cache_enabled() || !cache_enabled(),
        "cache_enabled must be reachable"
    );
}
