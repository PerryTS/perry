use super::*;

pub(super) struct FamilyDiag {
    pub(super) births: std::sync::atomic::AtomicU64,
    pub(super) dirty: std::sync::atomic::AtomicU64,
    pub(super) seen: std::sync::atomic::AtomicU64,
    pub(super) fills: std::sync::atomic::AtomicU64,
    pub(super) out_of_order: std::sync::atomic::AtomicU64,
    pub(super) other: std::sync::atomic::AtomicU64,
    pub(super) installs: std::sync::atomic::AtomicU64,
    pub(super) declined: std::sync::atomic::AtomicU64,
    pub(super) variant_births: std::sync::atomic::AtomicU64,
}

pub(super) static FAMILY_DIAG: FamilyDiag = FamilyDiag {
    births: std::sync::atomic::AtomicU64::new(0),
    dirty: std::sync::atomic::AtomicU64::new(0),
    seen: std::sync::atomic::AtomicU64::new(0),
    fills: std::sync::atomic::AtomicU64::new(0),
    out_of_order: std::sync::atomic::AtomicU64::new(0),
    other: std::sync::atomic::AtomicU64::new(0),
    installs: std::sync::atomic::AtomicU64::new(0),
    declined: std::sync::atomic::AtomicU64::new(0),
    variant_births: std::sync::atomic::AtomicU64::new(0),
};

/// `PERRY_FAMILY_DIAG=1`: print the family store counters at exit.
pub(crate) fn family_diag_init() {
    per_test_global! {
        static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    }
    extern "C" fn report() {
        family_diag_report();
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some()
        && !ON.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        RUN_DIAG_ON.store(true, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: registering a plain extern "C" function.
        unsafe { libc::atexit(report) };
    }
}

/// `PERRY_FAMILY_DIAG=2`: count out-of-order fills by (family, key written,
/// first later key present) and print the most frequent at exit (measurement
/// only: which key orders the census gets wrong).
pub(crate) fn family_diag_pairs_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var("PERRY_FAMILY_DIAG").as_deref() == Ok("2"))
}

pub(super) static OUT_OF_ORDER_PAIRS: std::sync::Mutex<
    Option<std::collections::HashMap<(u32, String, String), u64>>,
> = std::sync::Mutex::new(None);

#[cold]
pub(crate) unsafe fn note_out_of_order_pair(obj: *const ObjectHeader, pos: u32) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let name = |k: &PerryLayoutKey| {
        atom_bytes(k.atom)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    };
    let Some(later) = keys[pos as usize + 1..]
        .iter()
        .find(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE)
    else {
        return;
    };
    let key = (d.class_id, name(&keys[pos as usize]), name(later));
    if let Ok(mut map) = OUT_OF_ORDER_PAIRS.lock() {
        *map.get_or_insert_with(Default::default)
            .entry(key)
            .or_default() += 1;
    }
}

/// `PERRY_FAMILY_DIAG`: count the constructor runs' checks by outcome
/// (`layout_probe::perry_family_run_admits`).
pub(crate) static RUN_DIAG_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) static RUN_DIAG: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];

/// The outcomes [`RUN_DIAG`] counts.
pub(crate) const RUN_OUTCOMES: [&str; 8] = [
    "admitted",
    "not_family",
    "class",
    "key",
    "flags",
    "order",
    "chain",
    "record",
];

/// Count a run check's outcome `i` ([`RUN_OUTCOMES`]) under `PERRY_FAMILY_DIAG`.
#[inline(always)]
pub(crate) fn note_run(i: usize) {
    if RUN_DIAG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        RUN_DIAG[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) fn family_diag_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let runs: Vec<String> = RUN_OUTCOMES
        .iter()
        .zip(RUN_DIAG.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG runs {}", runs.join(" "));
    if family_diag_pairs_on() {
        if let Ok(map) = OUT_OF_ORDER_PAIRS.lock() {
            let mut pairs: Vec<_> = map.iter().flatten().collect();
            pairs.sort_by(|a, b| b.1.cmp(a.1));
            for ((class_id, key, later), n) in pairs.into_iter().take(40) {
                eprintln!(
                    "PERRY_FAMILY_DIAG out_of_order class={class_id} key={key} after={later} n={n}"
                );
            }
        }
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some() {
        family_diag_other_report();
        eprintln!(
            "PERRY_FAMILY_DIAG births={} variant_births={} dirty={} seen={} fills={} out_of_order={} other={} overflow_installs={} declined={}",
            FAMILY_DIAG.births.load(Relaxed),
            FAMILY_DIAG.variant_births.load(Relaxed),
            FAMILY_DIAG.dirty.load(Relaxed),
            FAMILY_DIAG.seen.load(Relaxed),
            FAMILY_DIAG.fills.load(Relaxed),
            FAMILY_DIAG.out_of_order.load(Relaxed),
            FAMILY_DIAG.other.load(Relaxed),
            FAMILY_DIAG.installs.load(Relaxed),
            FAMILY_DIAG.declined.load(Relaxed)
        );
    }
}

// --- overflow install causes (PERRY_FAMILY_DIAG, measurement only) ---------

/// Why the next `overflow::ensure` on a family object installs its table.
pub(crate) mod install_cause {
    pub const LATE_OUT_OF_ORDER: u8 = 1;
    pub const LATE_PROTO_OR_SPILL: u8 = 2;
    pub const OTHER_KEY: u8 = 3;
    pub const DEFINE: u8 = 4;
    pub const DELETE: u8 = 5;
    pub const DELETE_HOLE: u8 = 6;
    pub const UNIQUE_RUN: u8 = 7;
    pub const NAMES: [&str; 8] = [
        "unknown",
        "late_ooo",
        "late_proto_spill",
        "other_key",
        "define",
        "delete",
        "delete_hole",
        "unique_run",
    ];
}

thread_local! {
    static INSTALL_CAUSE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

pub(super) static INSTALLS_BY_CAUSE: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];

pub(crate) fn family_diag_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var_os("PERRY_FAMILY_DIAG").is_some())
}

#[inline(always)]
pub(crate) fn note_install_cause(cause: u8) {
    INSTALL_CAUSE.with(|c| c.set(cause));
}

/// `overflow::ensure` is installing a table on `obj`: count it by cause when
/// `obj` is a family object.
pub(crate) unsafe fn diag_note_install(obj: *const ObjectHeader) {
    let cause = INSTALL_CAUSE.with(|c| c.replace(0));
    if family_diag_on() && family_desc_of(obj).is_some() {
        INSTALLS_BY_CAUSE[cause as usize & 7].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// (class id, key) -> (first installs, all stores) of keys a family layout
/// does not name.
pub(super) static OTHER_KEYS: std::sync::Mutex<
    Option<std::collections::HashMap<(u32, String), (u64, u64)>>,
> = std::sync::Mutex::new(None);

#[cold]
pub(super) unsafe fn note_other_key(obj: *const ObjectHeader, key_bits: u64, first: bool) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    if let Ok(mut map) = OTHER_KEYS.lock() {
        let e = map
            .get_or_insert_with(Default::default)
            .entry((d.class_id, name))
            .or_default();
        e.0 += first as u64;
        e.1 += 1;
    }
}

pub(super) fn family_diag_other_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let causes: Vec<String> = install_cause::NAMES
        .iter()
        .zip(INSTALLS_BY_CAUSE.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG install_causes {}", causes.join(" "));
    if std::env::var("PERRY_FAMILY_DIAG").as_deref() != Ok("3") {
        return;
    }
    if let Ok(map) = OTHER_KEYS.lock() {
        let mut rows: Vec<_> = map.iter().flatten().collect();
        let mut by_class: std::collections::HashMap<u32, (u64, u64, u32)> = Default::default();
        for ((c, _), (f, t)) in &rows {
            let e = by_class.entry(*c).or_default();
            e.0 += f;
            e.1 += t;
            e.2 += 1;
        }
        let mut classes: Vec<_> = by_class.into_iter().collect();
        classes.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        for (c, (f, t, n)) in classes.into_iter().take(20) {
            let name = super::super::class_name_for_id(c).unwrap_or_default();
            eprintln!(
                "PERRY_FAMILY_DIAG other_class class={c} name={name} first={f} stores={t} keys={n}"
            );
        }
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.1 .1.cmp(&a.1 .1)));
        for ((c, k), (f, t)) in rows.into_iter().take(80) {
            eprintln!("PERRY_FAMILY_DIAG other class={c} key={k} first={f} stores={t}");
        }
    }
}
