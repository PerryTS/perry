//! Leaf fidelity and state-machine tests for immutable global transfer.

use super::*;

const NUMBER_ONE: u64 = 0x3FF0_0000_0000_0000;

struct Binding {
    cell: &'static AtomicUsize,
    canonical: &'static AtomicU64,
}

fn binding(initial: u64) -> Binding {
    Binding {
        cell: Box::leak(Box::new(AtomicUsize::new(0))),
        canonical: Box::leak(Box::new(AtomicU64::new(initial))),
    }
}

fn agent_cache() -> *mut AgentGlobalCache {
    Box::leak(Box::new(AgentGlobalCache {
        value: 0,
        state: STATE_UNSEEN,
    }))
}

fn addr<T>(p: *const T) -> i64 {
    p as i64
}

unsafe fn publish(b: &Binding, value: u64) -> *mut AgentGlobalCache {
    b.canonical.store(value, Ordering::Relaxed);
    let owner = agent_cache();
    js_thread_global_publish(addr(b.cell), addr(owner), addr(b.canonical));
    owner
}

unsafe fn read(b: &Binding, cache: *mut AgentGlobalCache) -> u64 {
    js_thread_global_materialize(addr(b.cell), addr(cache), addr(b.canonical)).to_bits()
}

unsafe fn string_view(bits: u64) -> (u32, u32, u32, Vec<u8>) {
    assert_eq!(bits & !POINTER_MASK, STRING_TAG);
    let h = (bits & POINTER_MASK) as *const StringHeader;
    let data = crate::string::string_data(h);
    let bytes = std::slice::from_raw_parts(data, (*h).byte_len as usize).to_vec();
    ((*h).utf16_len, (*h).flags, (*h).refcount, bytes)
}

unsafe fn owner_string(bytes: &[u8], wtf8: bool) -> u64 {
    let h = if wtf8 {
        crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    } else {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    };
    (*h).refcount = 1;
    STRING_TAG | (h as u64 & POINTER_MASK)
}

#[test]
fn pending_reads_stay_undefined_and_retry_after_publication() {
    unsafe {
        let b = binding(TAG_UNDEFINED);
        let worker = agent_cache();
        assert_eq!(read(&b, worker), TAG_UNDEFINED);
        assert_eq!((*worker).state, STATE_REGISTERED_PENDING);
        assert_eq!(read(&b, worker), TAG_UNDEFINED);
        assert_eq!(
            (*worker).state,
            STATE_REGISTERED_PENDING,
            "undefined is never cached as ready"
        );

        let owner_value = owner_string(b"late-initialized-leaf-value-longer-than-sso", false);
        publish(&b, owner_value);
        let local = read(&b, worker);
        assert_eq!((*worker).state, STATE_LOCAL_LEAF_READY);
        assert_ne!(local, owner_value, "the reader gets its own object");
        assert_eq!(string_view(local).3, string_view(owner_value).3);
    }
}

#[test]
fn owner_cache_names_the_canonical_object_and_marks_it_shared() {
    unsafe {
        let b = binding(TAG_UNDEFINED);
        let value = owner_string(b"owner-value-0123456789", false);
        let owner = publish(&b, value);
        assert_eq!((*owner).state, STATE_LOCAL_LEAF_READY);
        assert_eq!((*owner).value, value);
        assert_eq!(read(&b, owner), value);
        assert_eq!(
            string_view(value).2,
            0,
            "owner copy must not be an in-place append target"
        );
    }
}

#[test]
fn string_replicas_preserve_bytes_utf16_length_and_lone_surrogates() {
    unsafe {
        // ASCII, astral (U+1F600 = 2 UTF-16 units) and a lone surrogate U+D800.
        let cases: [(&[u8], bool); 4] = [
            (b"", false),
            ("a\u{1F600}b-astral-payload".as_bytes(), false),
            (&[b'x', 0xED, 0xA0, 0x80, b'y'], true),
            (&[0xED, 0xB0, 0x80], true),
        ];
        for (bytes, wtf8) in cases {
            let b = binding(TAG_UNDEFINED);
            let owner_value = owner_string(bytes, wtf8);
            publish(&b, owner_value);
            let local = read(&b, agent_cache());
            assert_ne!(local, owner_value);
            let (o16, oflags, _, obytes) = string_view(owner_value);
            let (l16, lflags, lref, lbytes) = string_view(local);
            assert_eq!(lbytes, obytes);
            assert_eq!(l16, o16);
            assert_eq!(
                lflags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
                oflags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES
            );
            assert_eq!(lref, 0);
            if wtf8 {
                assert_ne!(lflags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES, 0);
            }
        }
    }
}

#[test]
fn bigint_replicas_preserve_sign_zero_and_every_limb() {
    unsafe {
        let mut negative = [u64::MAX; BIGINT_LIMBS];
        negative[0] = u64::MAX - 41; // -42 in two's complement
        let mut wide = [0u64; BIGINT_LIMBS];
        for (i, limb) in wide.iter_mut().enumerate() {
            *limb = 0x0123_4567_89AB_CDEF ^ (i as u64);
        }
        for limbs in [[0u64; BIGINT_LIMBS], negative, wide] {
            let b = binding(TAG_UNDEFINED);
            let owner_ptr = bigint::bigint_alloc_with_limbs(limbs);
            let owner_value = BIGINT_TAG | (owner_ptr as u64 & POINTER_MASK);
            publish(&b, owner_value);
            let local = read(&b, agent_cache());
            assert_eq!(local & !POINTER_MASK, BIGINT_TAG);
            assert_ne!(local, owner_value);
            let local_ptr = (local & POINTER_MASK) as *const BigIntHeader;
            assert_eq!((*local_ptr).limbs, limbs);
        }
    }
}

#[test]
fn nonleaf_values_keep_the_canonical_route() {
    // A number and a short (inline, header-free) string are immediates.
    let short_ab = crate::value::SHORT_STRING_TAG
        | (2u64 << crate::value::SHORT_STRING_LEN_SHIFT)
        | u64::from(u16::from_le_bytes(*b"ab"));
    for value in [NUMBER_ONE, short_ab] {
        unsafe {
            let b = binding(TAG_UNDEFINED);
            let owner = publish(&b, value);
            assert_eq!((*owner).state, STATE_CANONICAL_NONLEAF);
            let worker = agent_cache();
            assert_eq!(read(&b, worker), value);
            assert_eq!((*worker).state, STATE_CANONICAL_NONLEAF);
            assert_eq!(b.cell.load(Ordering::Acquire), NONLEAF_RECORD);
        }
    }
}
