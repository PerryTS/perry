//! Design step 4: link-time ("static") ShapeIds.
//!
//! The driver names every compiler-visible class birth shape BY CONTENT —
//! the packed key names, the key count, the birth live bound, class or
//! literal prototype, and the typed masks when the class has a typed layout —
//! and assigns each distinct content one id in the static band (a
//! power-of-two band sized to the program, within the reserved
//! `perry_abi::STATIC_SHAPE_ID_COUNT` ids from `SHAPE_ID_BASE`). Equal content
//! means an equal slot layout (and the same class id), which is exactly what
//! the runtime already shares, so one id can never name two layouts. Class
//! NAMES never identify a shape: an importer's stub keys can differ from the
//! definer's (#5094), and then the two contents simply get two ids.
//!
//! Adoption is the runtime's: module init hands the id to the ordinary mint
//! as `requested` (`js_object_shape_id_for_class_keys_static`,
//! `js_object_shape_id_for_class_keys`), which mints it on a by-facts miss and
//! otherwise returns the existing id; a refused id aborts (ids are by
//! content, so a refusal is an invariant violation). Births stamp what the
//! mint RETURNED; only guards compare against the static id as an immediate.
//!
//! The content of a birth comes from ONE function, [`class_birth`], which the
//! string pool uses to emit the mint and the driver's pre-pass uses (through
//! [`crate::module_birth_shapes`]) to collect every module's contents.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::runtime_abi::{SHAPE_ID_BASE, STATIC_SHAPE_ID_COUNT};

/// The prototype a birth shape names: the anonymous object-literal classes
/// (`__AnonShape_*`) register as plain literals (`proto_id = 0`); every other
/// compiled class names its class id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BirthProto {
    Literal,
    Class(u32),
}

/// A typed class layout's masks (#8405): part of the content, so two layouts
/// with equal keys but different slot representations get different ids.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypedMasks {
    pub raw_f64_words: Vec<u64>,
    pub pointer_words: Vec<u64>,
}

/// The content of one compiler-visible birth shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BirthShape {
    /// NUL-terminated key names, exactly the bytes the keys array is built from.
    pub keys: Vec<u8>,
    pub key_count: u32,
    /// Live inline bound at birth (`>= key_count`).
    pub live: u32,
    pub proto: BirthProto,
    pub typed: Option<TypedMasks>,
    /// The birth representation word (charter step 5, T1;
    /// `typed_shape::class_birth_rep_in`): `F64` lanes are shape identity at
    /// runtime, so they are content here. A class born with an `F64` lane and
    /// an importer's all-`Any` stub of the same keys are two contents, and
    /// the stub never adopts the definer's id.
    pub rep: u64,
}

impl BirthShape {
    /// A literal content without a typed layout: the runtime seed mints it
    /// from its key names and its birth rep (`js_shape_seed_plain`), the
    /// same facts the literal's own mint names, so the seed and a lazy mint
    /// are one ShapeId. Class contents are seeded by their class
    /// registration, typed ones by their typed install.
    pub fn is_seedable(&self) -> bool {
        self.proto == BirthProto::Literal && self.typed.is_none()
    }

    /// The facts the runtime mints for this content, without the masks: a
    /// typed layout and a structural mint of the same class share them. The
    /// rep is a runtime fact, so it is part of them.
    pub(crate) fn structure(&self) -> (&[u8], u32, u32, &BirthProto, u64) {
        (&self.keys, self.key_count, self.live, &self.proto, self.rep)
    }

    /// A stable 64-bit FNV-1a over the content (never `RandomState`: the id
    /// must be identical across builds so cached objects stay valid).
    fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        eat(&(self.keys.len() as u64).to_le_bytes());
        eat(&self.keys);
        eat(&self.key_count.to_le_bytes());
        eat(&self.live.to_le_bytes());
        match self.proto {
            BirthProto::Literal => eat(&[0]),
            BirthProto::Class(cid) => {
                eat(&[1]);
                eat(&cid.to_le_bytes());
            }
        }
        if let Some(masks) = &self.typed {
            eat(&[2]);
            for words in [&masks.raw_f64_words, &masks.pointer_words] {
                eat(&(words.len() as u64).to_le_bytes());
                for w in words {
                    eat(&w.to_le_bytes());
                }
            }
        }
        // An all-`Any` rep adds nothing, so contents without an `F64` lane
        // keep the ids they had before the rep was content.
        if self.rep != 0 {
            eat(&[3]);
            eat(&self.rep.to_le_bytes());
        }
        h
    }

    /// The content's home slot in a band of `band` slots (a power of two):
    /// the hash folded to 32 bits, masked.
    fn home_slot(&self, band: u32) -> u32 {
        let h = self.content_hash();
        ((h ^ (h >> 32)) as u32) & (band - 1)
    }
}

/// The smallest static band: programs with up to 256 contents all share it,
/// so small programs never renumber as they grow.
pub const MIN_STATIC_BAND: u32 = 1024;

/// The size of the static band for `count` distinct contents: the next power
/// of two at or above `4 * count`, at least [`MIN_STATIC_BAND`], at most the
/// reserved `STATIC_SHAPE_ID_COUNT`. The band therefore holds its contents at
/// a load between 1/8 and 1/4 (lower at the minimum), and only changes size
/// when the count crosses a power-of-two threshold.
pub fn static_band_size(count: usize) -> u32 {
    let want = count
        .saturating_mul(4)
        .max(MIN_STATIC_BAND as usize)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX);
    want.min(STATIC_SHAPE_ID_COUNT as usize) as u32
}

/// Assign every distinct content a static id in a band sized to the program:
/// `SHAPE_ID_BASE + slot`, where `slot` is the content's hash reduced to
/// [`static_band_size`]`(count)` slots, with linear probing on collision over
/// the contents in sorted order (so the result depends on the SET only).
///
/// Two properties, both needed:
///
/// * **Compact.** The runtime's by-id store (`ShapeSlab`) indexes a
///   two-level directory of 32-record chunks by id, so every 32-id run that
///   holds an id allocates and touches a chunk, and every 32 K-id run a
///   directory page. Hashing over the whole 2^20 reserve gave each id its own
///   chunk (tsc: ~1.5 MB at startup, more with transparent huge pages). A
///   band of 4-8 slots per content keeps the ids within `band / 32` chunks
///   and, for any band up to 32 K ids, one directory page.
/// * **Stable.** A content keeps its id when other contents are added or
///   removed, as long as the band size is unchanged and the change does not
///   land in its probe run: an added content can only displace the contents
///   after it (in sorted order) in the run of occupied slots it joins, and at
///   a load of at most 1/4 those runs are short. The object cache (whose key
///   includes each module's `(content, id)` pairs) rebuilds exactly the
///   modules whose ids moved. When the count crosses a power-of-two
///   threshold the band doubles and every id moves once.
///
/// Every distinct content gets its OWN id (decision 16): a structural view of
/// a class and its typed layout are different content, so they never share an
/// id and a guard immediate never names two layouts. An importer's
/// structural stub with exactly the definer's facts is the definer's content
/// at runtime and uses the definer's id through [`ProgramClassShapeIds`]
/// (its own entry here is then never requested).
///
/// Contents beyond the reserve's capacity get no id (their guards load the
/// mint's id, as before step 4).
pub fn assign_static_shape_ids<'a>(
    contents: impl IntoIterator<Item = &'a BirthShape>,
) -> HashMap<BirthShape, u32> {
    let contents: BTreeSet<&BirthShape> = contents.into_iter().collect();
    let band = static_band_size(contents.len());
    let mut used = vec![false; band as usize];
    let mut ids: HashMap<BirthShape, u32> = HashMap::with_capacity(contents.len());
    for c in contents {
        if ids.len() as u32 >= band {
            break;
        }
        let mut slot = c.home_slot(band);
        while used[slot as usize] {
            slot = (slot + 1) & (band - 1);
        }
        used[slot as usize] = true;
        ids.insert(c.clone(), SHAPE_ID_BASE + slot);
    }
    ids
}

/// One class keys global's birth as the driver's pre-pass collects it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleBirth {
    /// The module's keys global (`perry_class_keys_<prefix>__<class>`).
    pub keys_global: String,
    pub class_id: u32,
    /// The module DEFINES the class (false: an imported class's stub).
    pub defined: bool,
    pub shape: BirthShape,
}

/// A class's static id as its DEFINING module assigns it (typed or plain).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DefinedClassShape {
    pub keys_global: String,
    pub shape: BirthShape,
    pub id: u32,
}

/// Decision 16: the program-wide map of each class's static id, by class id,
/// as the defining module assigns it. The driver hands each module the
/// entries it can name (its classes and imported stubs, and the producer
/// classes of its short-spread candidates), and those entries are part of the
/// module's object-cache key. A class id defined by two modules (colliding
/// ids) has no entry: its importers' guards keep their own ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramClassShapeIds(pub BTreeMap<u32, DefinedClassShape>);

impl ProgramClassShapeIds {
    pub fn from_births<'a>(
        births: impl IntoIterator<Item = &'a ModuleBirth>,
        ids: &HashMap<BirthShape, u32>,
    ) -> Self {
        let mut by_class: BTreeMap<u32, Vec<DefinedClassShape>> = BTreeMap::new();
        for b in births.into_iter().filter(|b| b.defined && b.class_id != 0) {
            if let Some(&id) = ids.get(&b.shape) {
                by_class
                    .entry(b.class_id)
                    .or_default()
                    .push(DefinedClassShape {
                        keys_global: b.keys_global.clone(),
                        shape: b.shape.clone(),
                        id,
                    });
            }
        }
        Self(
            by_class
                .into_iter()
                .filter_map(|(cid, mut v)| (v.len() == 1).then(|| (cid, v.pop().unwrap())))
                .collect(),
        )
    }

    /// The entries for `class_ids` only (one module's slice).
    pub fn restricted_to(&self, class_ids: impl IntoIterator<Item = u32>) -> Self {
        Self(
            class_ids
                .into_iter()
                .filter_map(|cid| self.0.get(&cid).map(|d| (cid, d.clone())))
                .collect(),
        )
    }

    /// The static id of a keys global of THIS module holding `shape` (whose
    /// own content was assigned `own`): the id its mint REQUESTS and its
    /// guards embed. It is the defining module's id when this is a structural
    /// stub of exactly the definer's facts (keys, count, live bound,
    /// prototype): the runtime mints the same facts for both, so they are one
    /// content, and whichever module initializes first adopts the id — the
    /// definer's typed install accepts a record of its exact facts already
    /// under it — so init order never matters and no guard misses. Else
    /// `own`. A typed stub keeps its own id: its code may rely on its own
    /// masks, which the definer's layout need not share.
    pub(crate) fn resolved_id(
        &self,
        keys_global: &str,
        class_id: u32,
        shape: &BirthShape,
        own: u32,
    ) -> u32 {
        match self.0.get(&class_id) {
            Some(d)
                if d.keys_global != keys_global
                    && shape.typed.is_none()
                    && d.shape.structure() == shape.structure() =>
            {
                d.id
            }
            _ => own,
        }
    }
}

/// One class keys global's birth, as the string pool mints it.
pub(crate) struct ClassBirth {
    /// The class id the mint names (0 = none; such a birth has no content).
    pub class_id: u32,
    /// Live inline bound when the class is born wide, else 0.
    pub wide_live: u32,
    /// Its content, when it is nameable.
    pub shape: Option<BirthShape>,
}

/// `(keys global, packed names, field count, raw-f64 mask words, pointer mask
/// words)`, one per class keys global (`compile_module`).
pub(crate) type ClassKeysInit = (String, String, u32, Vec<u64>, Vec<u64>);

/// The birth of one class keys global: the ONE derivation both the string
/// pool's mint and the driver's content collection use.
pub(crate) fn class_birth(
    module_prefix: &str,
    entry: &ClassKeysInit,
    class_header_image_inits: &HashMap<String, (u32, u64, u32)>,
    class_birth_reps: &HashMap<String, u64>,
    class_ids: &HashMap<String, u32>,
) -> ClassBirth {
    let (global_name, packed, field_count, _raw_mask_words, _pointer_mask_words) = entry;
    // The global is `perry_class_keys_<modprefix>__<sanitized class>`. Several
    // names can sanitize alike; take the smallest name so the choice is
    // deterministic (the pre-pass and codegen must agree).
    let prefix = format!("perry_class_keys_{}__", module_prefix);
    let sanitized_class = global_name.strip_prefix(&prefix).unwrap_or("");
    let class_id = class_ids
        .iter()
        .filter(|(k, _)| super::helpers::sanitize(k) == sanitized_class)
        .min_by(|a, b| a.0.cmp(b.0))
        .map(|(_, &v)| v)
        .unwrap_or(0);
    let image = class_header_image_inits.get(global_name);
    let wide_live = match image {
        Some(&(_, _, birth_live)) if birth_live > *field_count => birth_live,
        _ => 0,
    };
    // An empty literal names the runtime's own empty shape, which the runtime
    // mints at startup before any seed or module init: it can never adopt a
    // static id (its mint would only ever by-facts hit the counter id), so it
    // is not given one.
    let literal = sanitized_class.starts_with("__AnonShape_");
    let shape = (class_id != 0 && !(literal && *field_count == 0)).then(|| BirthShape {
        keys: packed.as_bytes().to_vec(),
        key_count: *field_count,
        live: if wide_live > 0 {
            wide_live
        } else {
            *field_count
        },
        proto: if literal {
            BirthProto::Literal
        } else {
            BirthProto::Class(class_id)
        },
        typed: None,
        rep: class_birth_reps.get(global_name).copied().unwrap_or(0),
    });
    ClassBirth {
        class_id,
        wide_live,
        shape,
    }
}

thread_local! {
    /// This module's static id per class keys global: the id its mint
    /// requests and its guards compare as an immediate (the definer's id for
    /// a structural stub of the definer's facts, see
    /// [`ProgramClassShapeIds::resolved_id`]), with the global's birth
    /// content (the facts the id names: a resolved definer id names the same
    /// structure). Set by `compile_module` for every module (empty when the
    /// driver assigned none).
    static MODULE_STATIC_IDS: RefCell<HashMap<String, (u32, BirthShape)>> =
        RefCell::new(HashMap::new());
    /// The seedable static ids this module's GUARDS embedded, with their
    /// content: the module's part of the program's seed set. Cleared by
    /// [`set_module_static_ids`], drained by [`take_module_static_seeds`]
    /// right after `compile_module` on the same thread.
    static MODULE_SEEDS: RefCell<BTreeMap<u32, BirthShape>> = RefCell::new(BTreeMap::new());
    /// This module's slice of the program-wide map (foreign shape globals).
    static MODULE_PROGRAM_IDS: RefCell<ProgramClassShapeIds> = RefCell::new(ProgramClassShapeIds::default());
}

/// Install this module's static id maps: for each class keys global whose
/// content the driver assigned an id.
pub(crate) fn set_module_static_ids(
    module_prefix: &str,
    class_keys_init_data: &[ClassKeysInit],
    class_header_image_inits: &HashMap<String, (u32, u64, u32)>,
    class_birth_reps: &HashMap<String, u64>,
    class_ids: &HashMap<String, u32>,
    assigned: &[(BirthShape, u32)],
    program: &ProgramClassShapeIds,
) {
    let by_content: HashMap<&BirthShape, u32> = assigned.iter().map(|(c, id)| (c, *id)).collect();
    let map: HashMap<String, (u32, BirthShape)> = if by_content.is_empty() {
        HashMap::new()
    } else {
        class_keys_init_data
            .iter()
            .filter_map(|entry| {
                let birth = class_birth(
                    module_prefix,
                    entry,
                    class_header_image_inits,
                    class_birth_reps,
                    class_ids,
                );
                let shape = birth.shape.as_ref()?;
                let own = *by_content.get(shape)?;
                let id = program.resolved_id(&entry.0, birth.class_id, shape, own);
                Some((entry.0.clone(), (id, shape.clone())))
            })
            .collect()
    };
    MODULE_STATIC_IDS.with(|m| *m.borrow_mut() = map);
    MODULE_PROGRAM_IDS.with(|m| *m.borrow_mut() = program.clone());
    MODULE_SEEDS.with(|s| s.borrow_mut().clear());
}

/// Note that a guard embeds `id` as an immediate: a seedable content joins
/// this module's seed set.
fn note_guard_id(id: u32, seed: Option<&BirthShape>) {
    if let Some(shape) = seed.filter(|s| s.is_seedable()) {
        MODULE_SEEDS.with(|s| {
            s.borrow_mut().entry(id).or_insert_with(|| shape.clone());
        });
    }
}

/// Drain the seed set of the module just compiled on this thread: every
/// seedable static id its guards embedded, with its content. The driver
/// persists it beside the module's cached object, so a cache hit replays the
/// same set a cold build produced.
pub fn take_module_static_seeds() -> Vec<(u32, BirthShape)> {
    MODULE_SEEDS.with(|s| std::mem::take(&mut *s.borrow_mut()).into_iter().collect())
}

/// The version of the seed sidecar's line format ([`encode_static_seed`]),
/// part of the object-cache key: an entry written in another format is a
/// miss, never a line this decoder reads as other facts (a pinned
/// `PERRY_OBJECT_CACHE_BUILD_ID` keeps the build id across compilers).
pub const STATIC_SEED_FORMAT: &str = "2";

/// One seed as a line of the object cache's seed sidecar:
/// `<id> <key_count> <live> <hex of the NUL-terminated key names> <rep>`,
/// the rep as `0x`-prefixed hex. Every field the seed mints from is in the
/// line: a warm link seeds exactly the facts the cold one did.
pub fn encode_static_seed(id: u32, shape: &BirthShape) -> String {
    let hex: String = shape.keys.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{id} {} {} {hex} {:#x}",
        shape.key_count, shape.live, shape.rep
    )
}

/// The inverse of [`encode_static_seed`]; `None` for a malformed line
/// (including a line of another format, which lacks the rep field).
pub fn decode_static_seed(line: &str) -> Option<(u32, BirthShape)> {
    let mut it = line.split_ascii_whitespace();
    let id = it.next()?.parse().ok()?;
    let key_count = it.next()?.parse().ok()?;
    let live = it.next()?.parse().ok()?;
    let hex = it.next()?;
    let rep = u64::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
    if it.next().is_some() || hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    let keys = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    Some((
        id,
        BirthShape {
            keys,
            key_count,
            live,
            proto: BirthProto::Literal,
            typed: None,
            rep,
        },
    ))
}

/// The static id a GUARD compares against for the class whose keys global is
/// `keys_global`, when the driver assigned one and the global belongs to this
/// module. A literal content joins the module's seed set.
pub(crate) fn static_shape_id_for_keys_global(keys_global: &str) -> Option<u32> {
    MODULE_STATIC_IDS.with(|m| {
        let m = m.borrow();
        let (id, shape) = m.get(keys_global)?;
        note_guard_id(*id, Some(shape));
        Some(*id)
    })
}

/// The static supplier of a loop region (DESIGN §4.1): the static id of
/// `keys_global` and the inline slot of each of `keys` in the birth shape
/// that id names, when every key is an inline data slot the region word can
/// carry. The runtime packs a region word from the SHAPE (`region_loop_pack`:
/// first match among the inline keys, `slot < 32`); a birth shape's facts are
/// its keys in slot order with every key inline (`live >= key_count`), a
/// data summary, no holes and generation 0, so the slots follow from the
/// keys alone and this is the word the runtime would publish for the id.
/// `None` (the region keeps its learned supplier alone) when a key is not an
/// inline key of the birth shape, or when a key in `boxed_mask` (a bare
/// store of a value not proven a canonical double) sits on a non-`Any` lane
/// of the birth rep: the runtime's pack refuses that word too (charter step
/// 5). The third result is R in region-key order: only identity F64 lanes
/// of this exact birth ShapeId set a bit. A returned id is a guard immediate:
/// it joins the module's seed set like any other.
pub(crate) fn static_region_slots(
    keys_global: &str,
    keys: &[String],
    boxed_mask: u32,
) -> Option<(u32, Vec<u32>, u32)> {
    MODULE_STATIC_IDS.with(|m| {
        let m = m.borrow();
        let (id, shape) = m.get(keys_global)?;
        let names: Vec<&[u8]> = shape
            .keys
            .strip_suffix(&[0])
            .unwrap_or(&shape.keys)
            .split(|&b| b == 0)
            .collect();
        if names.len() != shape.key_count as usize || shape.live < shape.key_count {
            return None;
        }
        let slots = keys
            .iter()
            .map(|k| {
                let at = names.iter().position(|n| *n == k.as_bytes())?;
                (at < 32).then_some(at as u32)
            })
            .collect::<Option<Vec<u32>>>()?;
        let lane_is_any = |slot: u32| (shape.rep >> (2 * slot)) & 0b11 == 0;
        if slots
            .iter()
            .enumerate()
            .any(|(i, &slot)| boxed_mask & (1 << i) != 0 && !lane_is_any(slot))
        {
            return None;
        }
        let r_mask = slots.iter().enumerate().fold(0u32, |mask, (i, &slot)| {
            if (shape.rep >> (2 * slot)) & 0b11 == 0b01 {
                mask | (1 << i)
            } else {
                mask
            }
        });
        note_guard_id(*id, Some(shape));
        Some((*id, slots, r_mask))
    })
}

/// The static id this module's mint of `keys_global` requests (the same id
/// its guards embed; a mint alone does not need a seed).
pub(crate) fn requested_shape_id_for_keys_global(keys_global: &str) -> Option<u32> {
    MODULE_STATIC_IDS.with(|m| m.borrow().get(keys_global).map(|(id, _)| *id))
}

/// The static id behind ANOTHER module's shape-id global `shape_id_global`
/// for class `class_id`: the defining module's id, when the program map names
/// that class and the global is the definer's.
pub(crate) fn static_shape_id_for_foreign_global(
    class_id: u32,
    shape_id_global: &str,
) -> Option<u32> {
    MODULE_PROGRAM_IDS.with(|m| {
        let program = m.borrow();
        let d = program.0.get(&class_id)?;
        if crate::typed_shape::shape_id_global_name_from_keys_global(&d.keys_global)
            != shape_id_global
        {
            return None;
        }
        note_guard_id(d.id, Some(&d.shape));
        Some(d.id)
    })
}

/// The class births of one module (the driver's pre-pass). The first
/// `defined_len` entries of `class_keys_init_data` are the module's own
/// classes; the rest are imported stubs.
pub(crate) fn module_births(
    module_prefix: &str,
    class_keys_init_data: &[ClassKeysInit],
    defined_len: usize,
    class_header_image_inits: &HashMap<String, (u32, u64, u32)>,
    class_birth_reps: &HashMap<String, u64>,
    class_ids: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    class_keys_init_data
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let birth = class_birth(
                module_prefix,
                entry,
                class_header_image_inits,
                class_birth_reps,
                class_ids,
            );
            Some(ModuleBirth {
                keys_global: entry.0.clone(),
                class_id: birth.class_id,
                defined: i < defined_len,
                shape: birth.shape?,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "static_shape_ids_tests.rs"]
mod tests;
