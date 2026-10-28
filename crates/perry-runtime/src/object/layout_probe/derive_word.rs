use super::*;

pub(super) unsafe fn derive_word(rec: &ShapeRecord) -> u64 {
    let count = rec.logical_key_count;
    if !positional_layout(rec) {
        return PROBE_READY | PROBE_DECLINE;
    }
    let checked = if rec.summary()
        & (crate::object::key_attrs::SUMMARY_ACCESSOR | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
    {
        PROBE_ATTRS
    } else {
        0
    };
    let keys = rec.keys as usize as *const crate::array::ArrayHeader;
    if count == 0 {
        return PROBE_READY | PROBE_ATOMS | checked;
    }
    if keys.is_null() || !super::super::keys_prefix_is_immutable(keys) {
        return PROBE_READY | PROBE_DECLINE;
    }
    let words = crate::array::array_elements_ptr(keys) as *const u64;
    // Canonical words and their hashes: an atom or symbol as it is; a short
    // or plain string as its text's program atom when one exists (a site key
    // is one), else as a TEXT entry indexed under its text's hash, which the
    // cold path compares by text (an atom of that text may be created later:
    // a computed key interns one). So every list of strings and symbols
    // proves absence.
    let mut atoms = true;
    let mut aliased = false;
    let mut canon: Vec<(u64, Option<u64>, bool)> = Vec::with_capacity(count as usize);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..count as usize {
        let bits = *words.add(i);
        if bits == TAG_HOLE {
            canon.push((bits, None, false));
            continue;
        }
        if let Some(h) = key_word_hash(bits) {
            canon.push((bits, Some(h), false));
            continue;
        }
        let v = crate::JSValue::from_bits(bits);
        let Some(text) = crate::string::js_string_key_bytes(v, &mut sso) else {
            atoms = false;
            canon.push((bits, None, false));
            continue;
        };
        let h = crate::object::key_bytes_hash(text.as_ptr(), text.len());
        aliased = true;
        match crate::string::atom_lookup(text, h) {
            Some(a) if crate::string::is_program_atom(a) => {
                canon.push((STRING_TAG | a as u64, Some(h), false))
            }
            // A runtime object whose keys were installed as plain strings
            // before the program's atoms existed (the global object, the
            // startup namespaces) may answer reads outside its list (a lazy
            // global): its TEXT entries answer hits only. An ordinary layout
            // and `%Object.prototype%` (whose unlisted names the walk knows)
            // prove absence by text.
            _ if !text_proves_absence(rec) => {
                atoms = false;
                canon.push((bits, Some(h), true))
            }
            _ => canon.push((bits, Some(h), true)),
        }
    }
    let flags = PROBE_READY | checked | if atoms { PROBE_ATOMS } else { 0 };
    // A list with accessor or private entries is always indexed: its
    // entries carry which positions a hit may read with one load. So is one
    // that reads a key with one load the unindexed scan cannot
    // (`POSBOUND` is 0 for a list with optional keys, which the probe reads as holes): a
    // key stored at another slot than its position, or at its position past
    // the bound.
    let slotted = |i: usize| match canon[i] {
        (w, Some(_), false) if w == *words.add(i) => inline_slot(rec, keys, i as u32),
        _ => None,
    };
    let unbounded = |i: usize| {
        i as u32 >= rec.position_bound_raw()
            && (direct_position(rec, keys, i as u32) || slotted(i).is_some())
    };
    if count <= SCAN_KEYS && !aliased && checked == 0 && !(0..count as usize).any(unbounded) {
        return flags;
    }
    let buckets = (count as usize * 2).next_power_of_two();
    // The blocking entries' hashes (`KeyIndex::blocks`), every one of them.
    let mut blocking: Vec<u64> = Vec::new();
    let mut unknown = false;
    // `(position, inline slot)` of each public accessor whose pair is an
    // inline slot of every receiver ([`ACCESSOR_ENTRY`]), by position. An
    // accessor blocks a store, so the blocking pass below reads every
    // accessor's attributes once.
    let mut accessor_slots: Vec<(u32, u32)> = Vec::new();
    if rec.summary()
        & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
            | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
    {
        use crate::object::key_attrs::{ENTRY_ACCESSOR_MASK, ENTRY_NON_WRITABLE, ENTRY_PRIVATE};
        for (i, (w, h, text)) in canon.iter().enumerate() {
            if *w == TAG_HOLE {
                continue;
            }
            let entry = crate::object::key_attrs::keys_entry(keys, i as u32);
            if entry & (ENTRY_ACCESSOR_MASK | ENTRY_NON_WRITABLE | ENTRY_PRIVATE) == 0 {
                continue;
            }
            if entry & ENTRY_ACCESSOR_MASK != 0
                && entry & ENTRY_PRIVATE == 0
                && h.is_some()
                && !*text
                && *w == *words.add(i)
            {
                let slot = crate::object::key_attrs::key_storage_slot(keys, i as u32);
                if slot < rec.live_inline_slot_count && slot != NO_LOAD {
                    accessor_slots.push((i as u32, slot));
                }
            }
            match h {
                Some(h) => blocking.push(*h),
                None => {
                    unknown = true;
                    blocking.clear();
                    break;
                }
            }
        }
    }
    let layout = index_layout(count as usize, buckets, blocking.len());
    let table = std::alloc::alloc(layout) as *mut KeyIndex;
    if table.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    (*table).mask = (buckets - 1) as u32;
    (*table).count = count;
    (*table)._pad = 0;
    (*table).nblocks = if unknown {
        BLOCKS_UNKNOWN
    } else {
        blocking.len() as u32
    };
    let accessor_slotted = |i: usize| {
        accessor_slots
            .binary_search_by_key(&(i as u32), |&(pos, _)| pos)
            .ok()
            .map(|at| accessor_slots[at].1)
    };
    let block_out = KeyIndex::blocks(table) as *mut u64;
    for (i, h) in blocking.iter().enumerate() {
        *block_out.add(i) = *h;
    }
    let out = (*table).words.as_mut_ptr();
    for (i, _) in canon.iter().enumerate() {
        // An alias entry's word is its program atom; a TEXT entry's is its
        // text's hash, so a bucket compares the text only for a key of the
        // same hash.
        *out.add(i) = match canon[i] {
            (_, Some(h), true) => h,
            (w, _, _) if w != *words.add(i) => w,
            _ => match slotted(i) {
                Some(slot) => u64::from(slot),
                None if !accessor_slots.is_empty() => accessor_slotted(i).map_or(0, u64::from),
                None => 0,
            },
        };
    }
    let slots = KeyIndex::buckets(table) as *mut u32;
    for b in 0..buckets {
        *slots.add(b) = u32::MAX;
    }
    for (i, (_, h, text)) in canon.iter().enumerate() {
        let Some(h) = h else {
            continue;
        };
        let mut b = *h as usize & (buckets - 1);
        while *slots.add(b) != u32::MAX {
            b = (b + 1) & (buckets - 1);
        }
        let alias = canon[i].0 != *words.add(i);
        *slots.add(b) = i as u32
            | if *text { TEXT_ENTRY } else { 0 }
            | if alias { ALIAS_ENTRY } else { 0 }
            | if direct_position(rec, keys, i as u32) {
                DIRECT_ENTRY
            } else if slotted(i).is_some() {
                SLOT_ENTRY
            } else if !accessor_slots.is_empty() && accessor_slotted(i).is_some() {
                ACCESSOR_ENTRY
            } else {
                0
            };
    }
    table as u64 | flags
}
