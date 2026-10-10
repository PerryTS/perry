//! Diagnostic counts only: numeric sites and counts, no retained keys or objects.
use std::collections::BTreeMap;
use std::sync::Mutex;

per_test_global! {
    static COUNTS: Mutex<BTreeMap<u64, [u64; 4]>> = Mutex::new(BTreeMap::new());
}

#[inline]
pub(super) fn record(site: u64, key: u64) {
    if !crate::object::method_site::stats_report_enabled() {
        return;
    }
    let kind = unsafe {
        match key >> 48 {
            0x7FF9 => 1,
            0x7FFF => {
                let ptr = (key & crate::value::POINTER_MASK) as *const crate::StringHeader;
                let bytes = std::slice::from_raw_parts(
                    crate::string::string_data(ptr),
                    (*ptr).byte_len as usize,
                );
                let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                if crate::string::atom_lookup(bytes, hash) == Some(ptr) {
                    0
                } else {
                    2
                }
            }
            _ => 3,
        }
    };
    if let Ok(mut counts) = COUNTS.lock() {
        counts.entry(site).or_default()[kind] += 1;
    }
}

pub(crate) fn report() {
    if let Ok(counts) = COUNTS.lock() {
        for (site, c) in counts.iter() {
            eprintln!(
                "[key-census] site={site} atom={} sso={} heap={} other={}",
                c[0], c[1], c[2], c[3]
            );
        }
    }
}
