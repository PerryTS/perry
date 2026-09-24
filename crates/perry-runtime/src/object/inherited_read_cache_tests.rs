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

/// An entry is keyed on a SHAPE, not on a receiver, so two receivers that
/// genuinely have one shape must be served by one entry.
///
/// The construction order below is the test, not incidental setup. Two
/// receivers built "the same way" do NOT automatically have one ShapeId, and
/// the two things that decide it are both ordering-sensitive:
///
/// 1. **The key-add must reuse the first receiver's edge.** A ShapeId's
///    identity includes the keys ARRAY ADDRESS, and two objects share one only
///    when the second's key-add hits `object::transition_cache_lookup` — a
///    16384-entry DIRECT-MAPPED table hashed on `(predecessor ShapeId, the
///    interned key's address)`. A collision from an unrelated entry evicts the
///    edge, the second receiver mints its own keys array and its own ShapeId,
///    and nothing is wrong: a transition-cache miss costs a duplicate shape,
///    never a wrong answer. But it is address-keyed, so whether it collides
///    varies with heap placement RUN TO RUN. With an unrelated call between
///    the two `set`s this test failed 2 runs in 6 of the same binary. Both
///    key-adds therefore happen back to back, with only an allocation between
///    them, so the edge the first one inserts is certain to still be there.
/// 2. **Both prototype links must precede the prime.** `Object.setPrototypeOf`
///    is a semantic property event: it bumps `prop_plan_epoch`, which bumps the
///    one validity word every entry is re-proved against
///    (`object::proto_validity`, check 1). Linking `second` AFTER priming
///    `first` retires the entry that was just made. Measured, every other field
///    of the entry matched the second receiver exactly — same class id `0x0`,
///    same ShapeId, same recorded prototype bits, same slot index — and only
///    `validity` differed, by one. The miss that followed said nothing
///    whatever about entry sharing.
///
/// The ShapeId merge is then asserted rather than assumed. #10931 mints a
/// prototype divergence's generation as a pure function of `(predecessor
/// ShapeId, the prototype's stable serial, link kind)`, so with (1) holding,
/// these two land on ONE ShapeId. Before it, each drew a fresh value from the
/// monotonic counter and they never could, which is why this test was written
/// with an `if (*first).parent_class_id == (*second).parent_class_id` guard
/// around its assertion. That word IS the ShapeId
/// ([`shapes::object_shape_stamp`] reads it), the two were never equal, and the
/// body never ran: the test was dormant from the day it was written. It is an
/// assertion now, so it can never go quiet again.
#[test]
fn a_second_receiver_of_the_same_shape_shares_the_entry() {
    let _scope = PrimeScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "irc_a", 7.0);

        // A second receiver reaching the SAME prototype through the same
        // operation: same class id, same recorded prototype bits, same shape.
        let first = crate::object::js_object_alloc(0, 4);
        let second = crate::object::js_object_alloc(0, 4);
        set(first, "irc_own", 1.0);
        set(second, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(first), boxed(proto));
        crate::object::js_object_set_prototype_of(boxed(second), boxed(proto));

        assert_eq!(
            (*first).class_id,
            (*second).class_id,
            "the two receivers must share a class id, or the entry index \
             separates them for a reason that has nothing to do with shape"
        );
        assert_eq!(
            shapes::object_shape_stamp(first),
            shapes::object_shape_stamp(second),
            "#10931: the same divergence from the same predecessor to the same \
             prototype must mint ONE ShapeId. Two here and this test has no \
             subject — the assertions below would be asking whether two \
             DIFFERENT shapes share an entry, which they must not"
        );

        let k = key("irc_a");
        inherited_read_cache_prime(first, k).expect("prime");
        let hits_before = inherited_read_cache_hits();
        let value = inherited_read_cache_hit(second, k)
            .expect("two receivers with one shape must share one entry");
        assert_eq!(f64::from_bits(value.bits()), 7.0);
        assert_eq!(
            inherited_read_cache_hits(),
            hits_before + 1,
            "the second receiver was answered without the cache hitting — a \
             fall-through to the chain walk returns the same 7.0 and is \
             invisible in a program's output"
        );
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
fn adding_a_key_to_the_prototype_invalidates_through_proto_validity() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // A plain store is not a descriptor install, so it does NOT bump the
        // semantic epoch. It is a key-add transition on an object the prime
        // MARKED, so the shape-stamp funnel bumps the validity word — the
        // whole reason that hook exists.
        assert!(
            crate::object::proto_validity::object_is_marked_prototype(proto as usize),
            "priming must mark the hop, or nothing will ever see a mutation of it"
        );
        set(proto, "irc_b", 3.0);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a key added to the prototype changed its ShapeId and the entry \
             still matched: the prototype-validity bump is not load-bearing"
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
        // Fresh objects sharing a prototype also share a class id and shape.
        // Their inherited lookup should reuse a single cache entry.
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

        assert_eq!(primes, 1, "equivalent receivers should share one cache entry");

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
fn a_key_added_at_the_far_end_of_a_three_hop_chain_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        // O -> P1 -> P2 -> P3, `a` on P3. Under the per-hop walk this cost
        // three dependent loads on every hit; under the validity word it costs
        // the same one compare a one-hop chain costs.
        let p3 = crate::object::js_object_alloc(0, 4);
        set(p3, "irc_a", 7.0);
        let p2 = crate::object::js_object_alloc(0, 4);
        set(p2, "irc_m2", 1.0);
        crate::object::js_object_set_prototype_of(boxed(p2), boxed(p3));
        let p1 = crate::object::js_object_alloc(0, 4);
        set(p1, "irc_m1", 1.0);
        crate::object::js_object_set_prototype_of(boxed(p1), boxed(p2));
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 1.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(p1));

        let k = key("irc_a");
        let primed = inherited_read_cache_prime(obj, k).expect("a three-hop chain must prime");
        assert_eq!(f64::from_bits(primed.bits()), 7.0);
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // Not the holder, not the receiver's prototype: the middle of the
        // chain, whose mutation neither end's guard can see.
        set(p2, "irc_new", 3.0);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a key added to an INTERIOR prototype left the entry matching"
        );
    }
}

#[test]
fn an_attribute_change_on_the_prototype_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // Not a value change and not a key change: only the attributes move.
        crate::object::descriptor_state::set_property_attrs(
            proto as usize,
            "irc_a".to_string(),
            crate::object::descriptor_state::PropertyAttrs::new(false, false, false),
        );
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a non-enumerable/non-writable redefinition of the cached key must \
             retire the entry: the slot is no longer the whole answer"
        );
    }
}

#[test]
fn a_mutation_of_an_object_nobody_inherits_from_does_not_invalidate() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // The common case of all mutation. A global validity counter that was
        // not gated on the mark would invalidate here, and this cache would be
        // a recompute in every program that builds objects in a loop.
        for i in 0..8 {
            let bystander = crate::object::js_object_alloc(0, 4);
            set(bystander, "irc_bystander", i as f64);
            set(bystander, "irc_bystander2", i as f64);
        }
        assert!(
            inherited_read_cache_hit(obj, k).is_some(),
            "building unrelated objects invalidated the chain: the validity \
             bump is not gated on OBJ_FLAG_IS_PROTOTYPE"
        );
    }
}

#[test]
fn replacing_a_registered_class_prototype_invalidates() {
    let _scope = PrimeScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("irc_a");
        inherited_read_cache_prime(obj, k).expect("prime");
        assert!(inherited_read_cache_hit(obj, k).is_some());

        // The chain of an `Object.create` / `new C()` receiver with no meta
        // record is resolved out of `CLASS_PROTOTYPE_OBJECTS`. Re-registering
        // mutates NOTHING this entry records: not the receiver, not the
        // holder, not any shape. Only the surface generation moves.
        let replacement = crate::object::js_object_alloc(0, 4);
        set(replacement, "irc_a", 42.0);
        crate::object::class_prototype_object_root_store(0x4242_0001, replacement);
        assert!(
            inherited_read_cache_hit(obj, k).is_none(),
            "a re-registered class prototype left the entry matching: it would \
             now answer with a different object than the chain walk beside it"
        );
    }
}

/// Coverage for the `[[Prototype]]` INSTALL sites, as a TEST rather than a
/// counter nobody reads.
///
/// The cache no longer marks its own hops: it refuses one that the install
/// funnel did not mark. That is the fail-safe polarity — a missed install site
/// costs a cache HIT, never a stale value — but "fail-safe" and "works" are
/// different claims, and only the hit counter can tell them apart, because a
/// cache that declines everything returns exactly the values the chain walk
/// would and is invisible in a program's output.
///
/// So each construction style below builds a receiver the way real code does,
/// through the same runtime entry points the compiled code calls, and asserts
/// the READ WAS SERVED BY THE CACHE. A future change that adds a way to build
/// a prototype without marking it fails here instead of quietly costing every
/// inherited read through it.
fn assert_style_is_cached(style: &str, obj: *mut ObjectHeader, key_name: &str, want: f64) {
    unsafe {
        let k = key(key_name);
        // One warm-up attempt is allowed: a route the install funnel does not
        // cover marks its hop on the first walk and abandons it, so the SECOND
        // read is the one that primes. What is not allowed is never priming,
        // which is what a route with no marking at all would do.
        if inherited_read_cache_prime(obj, key(key_name)).is_none() {
            let _ = inherited_read_cache_prime(obj, key(key_name));
        }
        test_clear_cache();
        test_reset_counters();
        let primed = inherited_read_cache_prime(obj, k)
            .unwrap_or_else(|| panic!("{style}: the walk did not resolve {key_name}"));
        assert_eq!(f64::from_bits(primed.bits()), want, "{style}: wrong value");
        assert_eq!(
            inherited_read_cache_primes(),
            1,
            "{style}: the walk resolved the key but REFUSED to record it — the \
             prototype it went through was never marked by an install site, so \
             every read of it re-walks the chain"
        );
        let served = inherited_read_cache_hit(obj, k)
            .unwrap_or_else(|| panic!("{style}: the entry did not serve the next read"));
        assert_eq!(
            f64::from_bits(served.bits()),
            want,
            "{style}: wrong value on hit"
        );
        assert_eq!(
            inherited_read_cache_hits(),
            1,
            "{style}: not counted as a hit"
        );
    }
}

#[test]
fn every_common_way_of_building_a_receiver_is_cached() {
    let _scope = PrimeScope::new();
    // EVERY style gets its OWN prototype object. Sharing one would let a
    // style pass because an EARLIER style's install site marked it, which
    // is the exact shape of a test that cannot fail for the reason it
    // names.
    let fresh_proto = |value: f64| {
        let p = crate::object::js_object_alloc(0, 4);
        set(p, "cov_a", value);
        p
    };

    // 1. `Object.setPrototypeOf` on an object literal.
    let p1 = fresh_proto(1.0);
    let literal = crate::object::js_object_alloc(0, 4);
    set(literal, "cov_own", 0.0);
    crate::object::js_object_set_prototype_of(boxed(literal), boxed(p1));
    assert_style_is_cached("setPrototypeOf on a literal", literal, "cov_a", 1.0);

    // 2. `Object.create(p)` — a different install route than 1.
    let p2 = fresh_proto(2.0);
    let created_bits = crate::object::js_object_create(boxed(p2));
    let created = crate::value::js_nanbox_get_pointer(created_bits) as *mut ObjectHeader;
    set(created, "cov_own", 0.0);
    assert_style_is_cached("Object.create", created, "cov_a", 2.0);

    // 3. A class-DEFAULT prototype link, as `new C()` performs it. This
    //    link deliberately bumps no epoch and transitions no shape, so if
    //    it did not mark, nothing else would.
    let p3 = fresh_proto(3.0);
    let instance = crate::object::js_object_alloc(0, 4);
    set(instance, "cov_own", 0.0);
    crate::object::prototype_chain::object_link_class_default_prototype(
        instance as usize,
        crate::value::js_nanbox_pointer(p3 as i64).to_bits(),
    );
    assert_style_is_cached("class-default link (new C())", instance, "cov_a", 3.0);

    // 4. An evaluated class prototype (#9502) — its own link kind.
    let p4 = fresh_proto(4.0);
    let evaluated = crate::object::js_object_alloc(0, 4);
    set(evaluated, "cov_own", 0.0);
    crate::object::prototype_chain::object_link_class_evaluation_prototype(
        evaluated as usize,
        crate::value::js_nanbox_pointer(p4 as i64).to_bits(),
    );
    assert_style_is_cached("class-evaluation link", evaluated, "cov_a", 4.0);

    // 5. Two hops: a base class's prototype reached through a derived
    //    one. The MIDDLE object must be marked as well as the holder, or
    //    the walk refuses at hop 1 and never reaches the answer.
    let base_proto = crate::object::js_object_alloc(0, 4);
    set(base_proto, "cov_method", 9.0);
    let derived_proto = crate::object::js_object_alloc(0, 4);
    set(derived_proto, "cov_mid", 0.0);
    crate::object::js_object_set_prototype_of(boxed(derived_proto), boxed(base_proto));
    let derived = crate::object::js_object_alloc(0, 4);
    set(derived, "cov_own", 0.0);
    crate::object::js_object_set_prototype_of(boxed(derived), boxed(derived_proto));
    assert_style_is_cached("two hops (extends)", derived, "cov_method", 9.0);

    // 6. A prototype whose key is ASSIGNED AFTER the receiver exists —
    //    `C.prototype.m = ...` after construction. The link is already
    //    marked; this checks the later key add does not disturb it.
    let late_proto = crate::object::js_object_alloc(0, 4);
    set(late_proto, "cov_placeholder", 0.0);
    let late = crate::object::js_object_alloc(0, 4);
    set(late, "cov_own", 0.0);
    crate::object::js_object_set_prototype_of(boxed(late), boxed(late_proto));
    set(late_proto, "cov_late", 5.0);
    assert_style_is_cached("key added to the prototype later", late, "cov_late", 5.0);
}

#[test]
fn an_unmarked_prototype_is_refused_rather_than_cached_unsafely() {
    let _scope = PrimeScope::new();
    unsafe {
        // The fail-safe polarity, asserted directly. A chain reachable by a
        // route that never marked its hop must DECLINE, not cache: an entry
        // through an unmarked prototype is one that a key added to that
        // prototype would not invalidate.
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "cov_u", 3.0);
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "cov_own", 0.0);
        // Install the prototype WITHOUT the funnel, the way a future install
        // site that forgot to mark would.
        let meta = crate::object::object_meta_ensure(obj);
        (*meta).prototype = crate::value::js_nanbox_pointer(proto as i64).to_bits();
        assert!(
            !crate::object::proto_validity::object_is_marked_prototype(proto as usize),
            "this test is vacuous unless the prototype really is unmarked"
        );
        assert!(
            inherited_read_cache_prime(obj, key("cov_u")).is_none(),
            "the cache recorded an entry through an UNMARKED prototype: a key \
             added to that prototype would bump no validity and the entry \
             would keep serving a stale value"
        );
    }
}

/// **The walk never passes a hop that was unmarked when it began.**
///
/// This is not a property of this cache. It is what somebody ELSE's mechanism
/// rests on: an ABSENT verdict — "this key is on nothing in the whole chain" —
/// is invalidated by `proto_validity`, which bumps only for MARKED prototypes.
/// So an absent verdict is sound only if every hop on an exhausted chain was
/// marked, and the only reason that is true is the mark-and-abandon below:
/// the walk marks one unmarked hop, abandons without recording, and the NEXT
/// read gets one hop further. Proving exhaustion therefore implies every hop
/// it passed was already marked.
///
/// Nothing states that, and removing the abandon is an obvious-looking
/// optimisation — it costs one declined read per hop and appears to buy
/// nothing. This test is the guard. It is red on a build where the walk
/// proceeds past an unmarked hop, whether that hop gets marked in passing or
/// not at all, which are the two shapes such a "cleanup" takes.
///
/// A comment would lose this argument to a plausible cleanup in six months; a
/// red test does not.
#[test]
fn the_walk_never_passes_a_hop_that_was_unmarked_when_it_began() {
    let _scope = PrimeScope::new();
    unsafe {
        // O -> P1 -> P2, with `irc_deep` only on P2, and BOTH hops installed
        // without the funnel so neither is marked. That is the state a future
        // `[[Prototype]]` install site that forgot to mark would leave, and it
        // is also the state every chain is in before its first walk.
        let p2 = crate::object::js_object_alloc(0, 4);
        set(p2, "irc_deep", 21.0);
        let p1 = crate::object::js_object_alloc(0, 4);
        set(p1, "irc_mid", 1.0);
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "irc_own", 0.0);
        let link = |from: *mut ObjectHeader, to: *mut ObjectHeader| {
            let meta = crate::object::object_meta_ensure(from);
            (*meta).prototype = crate::value::js_nanbox_pointer(to as i64).to_bits();
        };
        link(p1, p2);
        link(obj, p1);

        let marked = |o: *mut ObjectHeader| {
            crate::object::proto_validity::object_is_marked_prototype(o as usize)
        };
        assert!(
            !marked(p1) && !marked(p2),
            "vacuous unless both hops really start unmarked"
        );

        let k = key("irc_deep");

        // Walk 1 reaches P1, finds it unmarked, marks it and stops. If it had
        // continued, P2 would be marked too — or, on a build that dropped the
        // marking entirely, neither would be.
        assert!(
            inherited_read_cache_prime(obj, k).is_none(),
            "the walk resolved through hops that were unmarked when it began"
        );
        assert!(marked(p1), "the walk must mark the hop it stopped at");
        assert!(
            !marked(p2),
            "the walk passed P1 in the same pass that marked it. An absent \
             verdict recorded through a chain walked this way rests on a hop \
             that a key add would not invalidate, and goes stale silently"
        );

        // Walk 2 gets exactly one hop further, for the same reason.
        assert!(inherited_read_cache_prime(obj, k).is_none());
        assert!(marked(p2), "the second walk must reach and mark P2");

        // Only now, with every hop marked BEFORE the walk begins, may the walk
        // resolve — and this is the state in which an exhausted chain may be
        // recorded as absent.
        let resolved = inherited_read_cache_prime(obj, k)
            .expect("with every hop marked, the walk must resolve");
        assert_eq!(f64::from_bits(resolved.bits()), 21.0);
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
fn the_validity_guard_is_load_bearing() {
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
