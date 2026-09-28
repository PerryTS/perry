//! Design step 4: link-time ("static") ShapeIds.
//!
//! The driver names every compiler-visible class birth shape BY CONTENT —
//! the packed key names, the key count, the birth live bound, class or
//! literal prototype, and the typed masks when the class has a typed layout —
//! and assigns each distinct content one id in the static band
//! (`perry_abi::STATIC_SHAPE_ID_COUNT` ids from `SHAPE_ID_BASE`). Equal content
//! means an equal slot layout (and the same class id), which is exactly what
//! the runtime already shares, so one id can never name two layouts. Class
//! NAMES never identify a shape: an importer's stub keys can differ from the
//! definer's (#5094), and then the two contents simply get two ids.
//!
//! Adoption is the runtime's: module init hands the id to the ordinary mint
//! as `requested` (`js_object_shape_id_for_class_keys_static`,
//! `js_gc_typed_shape_id_for_keys`), which mints it on a by-facts miss and
//! otherwise returns the existing id. Births stamp what the mint RETURNED;
//! only guards compare against the static id as an immediate, so a declined
//! id makes a guard miss (slow), never alias.
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
}

impl BirthShape {
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
        h
    }

    /// The facts the runtime mints for this content, without the masks: a
    /// typed layout and a structural mint of the same class share them.
    pub(crate) fn structure(&self) -> (&[u8], u32, u32, &BirthProto) {
        (&self.keys, self.key_count, self.live, &self.proto)
    }
}

/// Assign every distinct content a static id: `SHAPE_ID_BASE + (hash & mask)`,
/// linear probing on collision, over the contents in sorted order (so the
/// result depends on the SET only). Hash collisions in the 2^20 band are
/// certain at program scale, which is why this is a whole-program pass.
///
/// Every distinct content gets its OWN id (decision 16): a structural view of
/// a class and its typed layout are different content, so they never share an
/// id and a guard immediate never names two layouts. An importer's guards
/// reach the definer's id through [`ProgramClassShapeIds`] instead.
///
/// Contents beyond the band's capacity get no id (their guards load the
/// mint's id, as before step 4).
pub fn assign_static_shape_ids<'a>(
    contents: impl IntoIterator<Item = &'a BirthShape>,
) -> HashMap<BirthShape, u32> {
    let contents: BTreeSet<&BirthShape> = contents.into_iter().collect();
    let mask = STATIC_SHAPE_ID_COUNT - 1;
    let mut used = vec![false; STATIC_SHAPE_ID_COUNT as usize];
    let mut ids: HashMap<BirthShape, u32> = HashMap::with_capacity(contents.len());
    for c in contents {
        if ids.len() as u32 >= STATIC_SHAPE_ID_COUNT {
            break;
        }
        let mut slot = (c.content_hash() as u32) & mask;
        while used[slot as usize] {
            slot = (slot + 1) & mask;
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

    /// The guard immediate for a keys global of THIS module holding `shape`
    /// (minted as `own`): the defining module's id when this is a structural
    /// stub of exactly the definer's facts — the definer's typed install goes
    /// to the front of the by-facts bucket, so this module's births reach it
    /// too — else `own`. A typed stub keeps its own id: its code may rely on
    /// its own masks, which the definer's layout need not share.
    fn guard_id(&self, keys_global: &str, class_id: u32, shape: &BirthShape, own: u32) -> u32 {
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
    /// The class has a typed layout (#8405): `js_gc_typed_shape_id_for_keys`.
    pub typed: bool,
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
    class_ids: &HashMap<String, u32>,
) -> ClassBirth {
    let (global_name, packed, field_count, raw_mask_words, pointer_mask_words) = entry;
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
    const GC_LAYOUT_AND_INTACT_MASK: u64 = 0xD000;
    const GC_SIDE_MASK_AND_INTACT: u64 = 0x9000;
    let image = class_header_image_inits.get(global_name);
    let typed = image.is_some_and(|&(_, packed, _)| {
        ((packed >> 16) & GC_LAYOUT_AND_INTACT_MASK) == GC_SIDE_MASK_AND_INTACT
    });
    let wide_live = match image {
        Some(&(_, _, birth_live)) if !typed && birth_live > *field_count => birth_live,
        _ => 0,
    };
    let shape = (class_id != 0).then(|| BirthShape {
        keys: packed.as_bytes().to_vec(),
        key_count: *field_count,
        live: if wide_live > 0 {
            wide_live
        } else {
            *field_count
        },
        proto: if sanitized_class.starts_with("__AnonShape_") {
            BirthProto::Literal
        } else {
            BirthProto::Class(class_id)
        },
        typed: typed.then(|| TypedMasks {
            raw_f64_words: raw_mask_words.clone(),
            pointer_words: pointer_mask_words.clone(),
        }),
    });
    ClassBirth {
        class_id,
        typed,
        wide_live,
        shape,
    }
}

thread_local! {
    /// This module's static ids per class keys global: `(mint id, guard id)`.
    /// The mint id is the one its own content was assigned (the string pool
    /// requests it); the guard id is what a guard compares as an immediate
    /// (the definer's id for a structural stub, see
    /// [`ProgramClassShapeIds::guard_id`]). Set by `compile_module` for every
    /// module (empty when the driver assigned none).
    static MODULE_STATIC_IDS: RefCell<HashMap<String, (u32, u32)>> = RefCell::new(HashMap::new());
    /// This module's slice of the program-wide map (foreign shape globals).
    static MODULE_PROGRAM_IDS: RefCell<ProgramClassShapeIds> = RefCell::new(ProgramClassShapeIds::default());
}

/// Install this module's static id maps: for each class keys global whose
/// content the driver assigned an id.
pub(crate) fn set_module_static_ids(
    module_prefix: &str,
    class_keys_init_data: &[ClassKeysInit],
    class_header_image_inits: &HashMap<String, (u32, u64, u32)>,
    class_ids: &HashMap<String, u32>,
    assigned: &[(BirthShape, u32)],
    program: &ProgramClassShapeIds,
) {
    let by_content: HashMap<&BirthShape, u32> = assigned.iter().map(|(c, id)| (c, *id)).collect();
    let map: HashMap<String, (u32, u32)> = if by_content.is_empty() {
        HashMap::new()
    } else {
        class_keys_init_data
            .iter()
            .filter_map(|entry| {
                let birth = class_birth(module_prefix, entry, class_header_image_inits, class_ids);
                let shape = birth.shape.as_ref()?;
                let own = *by_content.get(shape)?;
                let guard = program.guard_id(&entry.0, birth.class_id, shape, own);
                Some((entry.0.clone(), (own, guard)))
            })
            .collect()
    };
    MODULE_STATIC_IDS.with(|m| *m.borrow_mut() = map);
    MODULE_PROGRAM_IDS.with(|m| *m.borrow_mut() = program.clone());
}

/// The static id a GUARD compares against for the class whose keys global is
/// `keys_global`, when the driver assigned one and the global belongs to this
/// module.
pub(crate) fn static_shape_id_for_keys_global(keys_global: &str) -> Option<u32> {
    MODULE_STATIC_IDS.with(|m| m.borrow().get(keys_global).map(|&(_, guard)| guard))
}

/// The static id this module's mint of `keys_global` requests (its own
/// content's id).
pub(crate) fn static_mint_id_for_keys_global(keys_global: &str) -> Option<u32> {
    MODULE_STATIC_IDS.with(|m| m.borrow().get(keys_global).map(|&(own, _)| own))
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
        (crate::typed_shape::shape_id_global_name_from_keys_global(&d.keys_global)
            == shape_id_global)
            .then_some(d.id)
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
    class_ids: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    class_keys_init_data
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let birth = class_birth(module_prefix, entry, class_header_image_inits, class_ids);
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
