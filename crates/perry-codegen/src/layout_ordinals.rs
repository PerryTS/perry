//! Compiler-only whole-image layout plan. Never emitted as an ID-to-row table.
//!
//! Identity assignment stays in static_shape_ids. This plan assigns physical
//! static rows from full keys, independently of identity hash slots and section
//! order. Every package in an image receives the same Arc.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayoutKey {
    Native(String),
    Identity(u32, crate::BirthShape),
    Descriptor {
        module: String,
        symbol: String,
        content: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayoutRole {
    Native,
    ClassFamily,
    /// Runtime and realm descriptor values, directly indexed on first touch.
    LazyImage,
    OtherStatic,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrdinalInput {
    pub key: LayoutKey,
    pub requested_shape_id: u32,
    pub band: u8,
    pub proto_kind: u8,
    pub role: LayoutRole,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrdinalEntry {
    pub key: LayoutKey,
    pub ordinal: u32,
    pub requested_shape_id: u32,
    pub band: u8,
    pub proto_kind: u8,
    pub role: LayoutRole,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProgramLayoutOrdinals {
    pub entries: Vec<OrdinalEntry>,
    pub lazy_first: u32,
    pub lazy_count: u32,
    pub family_first: u32,
    pub family_count: u32,
    pub prototype_serial_end: u64,
}

impl ProgramLayoutOrdinals {
    /// Native inputs are already in ABI catalog order (intrinsics, weak
    /// collections, then sorted descriptors). No independently maintained count.
    pub fn assign(
        native: impl IntoIterator<Item = OrdinalInput>,
        program: impl IntoIterator<Item = OrdinalInput>,
    ) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut seen = BTreeMap::new();
        for input in native {
            if input.role != LayoutRole::Native || !matches!(input.key, LayoutKey::Native(_)) {
                return Err("native catalog contains a non-native entry".into());
            }
            if seen.insert(input.key.clone(), input.clone()).is_some() {
                return Err("duplicate native catalog key".into());
            }
            Self::append(&mut entries, input)?;
        }
        let lazy_first = entries.len() as u32;
        let mut sorted = BTreeMap::new();
        for input in program {
            if input.role == LayoutRole::Native || matches!(input.key, LayoutKey::Native(_)) {
                return Err("program entry cannot extend the native catalog".into());
            }
            if let LayoutKey::Identity(identity, _) = &input.key {
                if *identity == 0 || *identity != input.requested_shape_id {
                    return Err("identity key disagrees with requested identity".into());
                }
            }
            if let Some(previous) = sorted.insert(input.key.clone(), input.clone()) {
                if previous != input {
                    return Err("same full layout key has conflicting tags or role".into());
                }
            }
        }
        // A requested identity owns precisely one content.
        let mut identities = BTreeMap::new();
        for input in sorted.values() {
            if input.requested_shape_id != 0 {
                if let Some(old) = identities.insert(input.requested_shape_id, &input.key) {
                    if old != &input.key {
                        return Err("requested identity names distinct full contents".into());
                    }
                }
            }
        }
        for input in sorted.values().filter(|i| i.role == LayoutRole::LazyImage) {
            Self::append(&mut entries, input.clone())?;
        }
        let lazy_count = entries.len() as u32 - lazy_first;
        let family_first = entries.len() as u32;
        let family = |i: &&OrdinalInput| {
            i.role == LayoutRole::ClassFamily && i.band == 0 && i.proto_kind == 0
        };
        for input in sorted.values().filter(family) {
            Self::append(&mut entries, input.clone())?;
        }
        let family_count = entries.len() as u32 - family_first;
        for input in sorted
            .values()
            .filter(|i| !family(i) && i.role != LayoutRole::LazyImage)
        {
            Self::append(&mut entries, input.clone())?;
        }
        Ok(Self {
            entries,
            lazy_first,
            lazy_count,
            family_first,
            family_count,
            prototype_serial_end: perry_abi::intrinsic_serials::FIRST_DYNAMIC_PROTOTYPE_SERIAL,
        })
    }

    fn append(entries: &mut Vec<OrdinalEntry>, input: OrdinalInput) -> Result<(), String> {
        let ordinal = u32::try_from(entries.len()).map_err(|_| "layout row exhaustion")?;
        // Validate BEFORE inserting; no truncation or wrapping.
        encode_layout_word(ordinal, input.band, input.proto_kind)?;
        entries.push(OrdinalEntry {
            key: input.key,
            ordinal,
            requested_shape_id: input.requested_shape_id,
            band: input.band,
            proto_kind: input.proto_kind,
            role: input.role,
        });
        Ok(())
    }

    pub fn static_rows(&self) -> u32 {
        self.entries.len() as u32
    }

    pub fn entry(&self, key: &LayoutKey) -> Option<&OrdinalEntry> {
        self.entries.iter().find(|entry| &entry.key == key)
    }

    pub fn identity_word(&self, identity: u32) -> Option<u32> {
        self.entries
            .iter()
            .find(|entry| identity != 0 && entry.requested_shape_id == identity)
            .map(OrdinalEntry::word)
    }

    /// A carried family descriptor's immutable template word. Family emission
    /// always carries its word; OtherStatic inputs can also describe static
    /// metadata whose initializer is zero, so their presence alone is no proof.
    /// Imported symbols are eligible, but ambiguous spellings stay checked.
    pub(crate) fn descriptor_template_word(&self, symbol: &str) -> Option<u32> {
        let mut entries = self.entries.iter().filter(|entry| {
            matches!(
                &entry.key, LayoutKey::Descriptor { symbol: name, .. } if name == symbol
            )
        });
        let first = entries.next()?;
        if first.role != LayoutRole::ClassFamily {
            return None;
        }
        let word = first.word();
        entries
            .all(|other| other.role == LayoutRole::ClassFamily && other.word() == word)
            .then_some(word)
    }

    /// Exact stable cache input, including full keys (not just content hashes),
    /// header bounds, ABI version and the entire native catalog.
    pub fn cache_key(&self) -> String {
        format!(
            "layout-word-v{}:{self:?}",
            perry_abi::LAYOUT_WORD_ABI_VERSION
        )
    }

    /// The `PerryLayoutImageV1` initializer; `@perry_static_seeds` is the
    /// static seed unit's table (`stubs::static_shape_seed_ll_with_plan`).
    pub fn image_initializer(&self) -> String {
        let lazy = if self.lazy_count == 0 {
            "null"
        } else {
            "@perry_lazy_layouts"
        };
        format!(
            "{{ i32 {}, i32 {}, i32 {}, i32 {}, ptr @perry_static_seeds, i32 {}, i32 {}, ptr {lazy}, i64 {} }}",
            perry_abi::LAYOUT_WORD_ABI_VERSION,
            self.static_rows(),
            self.family_first,
            self.family_count,
            self.lazy_first,
            self.lazy_count,
            self.prototype_serial_end,
        )
    }
}

impl OrdinalEntry {
    pub fn word(&self) -> u32 {
        encode_layout_word(self.ordinal, self.band, self.proto_kind)
            .expect("validated ordinal entry")
    }
}

/// Checked compiler encoding using the shared perry-abi constants.
pub fn encode_layout_word(row: u32, band: u8, proto_kind: u8) -> Result<u32, String> {
    if row >= perry_abi::LAYOUT_ROW_LIMIT || band >= 3 || proto_kind >= 4 {
        return Err("invalid LayoutWord row, band or prototype kind".into());
    }
    Ok(perry_abi::LayoutWord::new(
        perry_abi::LayoutRow(row),
        u32::from(band),
        u32::from(proto_kind),
    )
    .expect("validated compiler word")
    .0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(symbol: &str, role: LayoutRole, kind: u8) -> OrdinalInput {
        OrdinalInput {
            key: LayoutKey::Descriptor {
                module: "pkg/module".into(),
                symbol: symbol.into(),
                content: symbol.as_bytes().to_vec(),
            },
            requested_shape_id: 0,
            band: 0,
            proto_kind: kind,
            role,
        }
    }

    #[test]
    fn exact_raw_word_fixtures_and_checked_limits() {
        for (row, band, kind, expected) in [
            (0, 0, 0, 0x80000000),
            (1, 0, 2, 0x88000001),
            (1, 0, 3, 0x8c000001),
            (1, 1, 1, 0x94000001),
            (0, 2, 1, 0xa4000000),
        ] {
            assert_eq!(encode_layout_word(row, band, kind).unwrap(), expected);
        }
        assert_eq!(
            encode_layout_word(0, 0, 0).unwrap() | 0x40000000,
            0xc0000000
        );
        for band in 0..3 {
            for kind in 0..4 {
                let word = encode_layout_word((1 << 26) - 1, band, kind).unwrap();
                assert_eq!(word & 0x03ffffff, (1 << 26) - 1);
                assert_eq!((word >> 28) & 3, u32::from(band));
                assert_eq!((word >> 26) & 3, u32::from(kind));
            }
        }
        assert!(encode_layout_word(1 << 26, 0, 0).is_err());
        assert!(encode_layout_word(0, 3, 0).is_err());
        assert!(encode_layout_word(0, 0, 4).is_err());
    }

    #[test]
    fn full_keys_sort_deduplicate_and_keep_family_range_contiguous() {
        let a = input("a", LayoutRole::OtherStatic, 0);
        let z = input("z", LayoutRole::ClassFamily, 0);
        let null = input("null", LayoutRole::ClassFamily, 2);
        let p = ProgramLayoutOrdinals::assign([], [a.clone(), z.clone(), null.clone(), a.clone()])
            .unwrap();
        let q = ProgramLayoutOrdinals::assign([], [null, z, a]).unwrap();
        assert_eq!(p, q);
        assert_eq!(p.family_count, 1);
        assert_eq!(p.entries[0].role, LayoutRole::ClassFamily);
        assert_eq!(
            p.entries.iter().map(|e| e.ordinal).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        let mut changed = p.clone();
        changed.entries[1].proto_kind = 1;
        assert_ne!(p.cache_key(), changed.cache_key());
    }

    #[test]
    fn lazy_descriptors_are_one_directly_indexed_span_before_class_families() {
        let a = input("a", LayoutRole::LazyImage, 0);
        let b = input("b", LayoutRole::LazyImage, 2);
        let c = input("c", LayoutRole::ClassFamily, 0);
        let d = input("d", LayoutRole::OtherStatic, 0);
        let plan = ProgramLayoutOrdinals::assign(native_inputs(), [d, b, c, a]).unwrap();
        let first = perry_abi::NATIVE_LAYOUT_ROWS;
        assert_eq!((plan.lazy_first, plan.lazy_count), (first, 2));
        assert_eq!((plan.family_first, plan.family_count), (first + 2, 1));
        assert_eq!(plan.entries[first as usize].role, LayoutRole::LazyImage);
        assert_eq!(plan.entries[first as usize + 1].proto_kind, 2);
        assert!(plan.image_initializer().contains("ptr @perry_lazy_layouts"));
        assert!(ProgramLayoutOrdinals::assign(native_inputs(), [])
            .unwrap()
            .image_initializer()
            .ends_with(&format!(
                "i32 0, ptr null, i64 {} }}",
                perry_abi::intrinsic_serials::FIRST_DYNAMIC_PROTOTYPE_SERIAL
            )));
    }

    #[test]
    fn conflicting_tags_fail_without_aliasing() {
        let a = input("same", LayoutRole::OtherStatic, 0);
        let mut b = a.clone();
        b.proto_kind = 1;
        assert!(ProgramLayoutOrdinals::assign([], [a, b]).is_err());
    }

    #[test]
    fn descriptor_content_is_not_just_symbol_or_hash() {
        let a = input("same", LayoutRole::OtherStatic, 0);
        let mut b = a.clone();
        if let LayoutKey::Descriptor { content, .. } = &mut b.key {
            content.push(0);
        }
        assert_eq!(
            ProgramLayoutOrdinals::assign([], [a, b])
                .unwrap()
                .static_rows(),
            2
        );
    }
}

/// One authoritative ABI catalog, including absent target variants.
pub fn native_inputs() -> Vec<OrdinalInput> {
    use perry_abi::*;
    let mut inputs = Vec::new();
    for index in 0..9 {
        let word = intrinsic_function_layout_word(index);
        inputs.push(OrdinalInput {
            key: LayoutKey::Native(format!("intrinsic_function_{index}")),
            requested_shape_id: if index == 4 {
                FUNCTION_DICTIONARY_SHAPE
            } else {
                FUNCTION_SHAPE_BASE + index
            },
            band: ((word & LAYOUT_BAND_MASK) >> LAYOUT_BAND_SHIFT) as u8,
            proto_kind: ((word & LAYOUT_KIND_MASK) >> LAYOUT_KIND_SHIFT) as u8,
            role: LayoutRole::Native,
        });
    }
    for name in ["WeakMap", "WeakSet"]
        .into_iter()
        .chain(NATIVE_LAYOUT_DESCRIPTORS.iter().copied())
    {
        inputs.push(OrdinalInput {
            key: LayoutKey::Native(name.into()),
            requested_shape_id: 0,
            band: 0,
            proto_kind: 0,
            role: LayoutRole::Native,
        });
    }
    assert_eq!(inputs.len(), NATIVE_LAYOUT_ROWS as usize);
    inputs
}

pub fn identity_input(shape: &crate::BirthShape, identity: u32) -> OrdinalInput {
    OrdinalInput {
        key: LayoutKey::Identity(identity, shape.clone()),
        requested_shape_id: identity,
        band: 0,
        proto_kind: match shape.proto {
            crate::BirthProto::Literal => 0,
            crate::BirthProto::Class(_) => 0,
            crate::BirthProto::Prototype(_, _) => 0,
        },
        role: LayoutRole::OtherStatic,
    }
}

#[derive(Default)]
struct ModulePlan {
    module: String,
    plan: std::sync::Arc<ProgramLayoutOrdinals>,
    collecting: bool,
    inputs: Vec<OrdinalInput>,
    emitted_requests: std::collections::BTreeSet<u32>,
}
thread_local! {
    static MODULE: std::cell::RefCell<ModulePlan> = Default::default();
}
pub(crate) fn begin_module(
    module: &str,
    plan: std::sync::Arc<ProgramLayoutOrdinals>,
    collecting: bool,
) {
    MODULE.with(|m| {
        *m.borrow_mut() = ModulePlan {
            module: module.into(),
            plan,
            collecting,
            inputs: Vec::new(),
            emitted_requests: Default::default(),
        }
    });
}
pub fn take_module_layout_inputs() -> Vec<OrdinalInput> {
    MODULE.with(|m| std::mem::take(&mut m.borrow_mut().inputs))
}
pub(crate) fn descriptor_word(
    symbol: &str,
    content: String,
    kind: u8,
    family: bool,
    carried: bool,
    identity: u32,
) -> u32 {
    MODULE.with(|m| {
        let mut m = m.borrow_mut();
        let input = OrdinalInput {
            key: LayoutKey::Descriptor {
                module: m.module.clone(),
                symbol: symbol.into(),
                content: content.into_bytes(),
            },
            requested_shape_id: identity,
            band: 0,
            proto_kind: kind,
            role: if family {
                LayoutRole::ClassFamily
            } else {
                LayoutRole::OtherStatic
            },
        };
        if m.collecting {
            m.inputs.push(input);
            return 0;
        }
        if !carried {
            return 0;
        }
        if identity != 0 {
            return m
                .plan
                .identity_word(identity)
                .expect("static descriptor identity in image plan");
        }
        m.plan
            .entry(&input.key)
            .unwrap_or_else(|| {
                panic!(
                    "descriptor full content in image plan: {symbol} {:?}",
                    String::from_utf8_lossy(match &input.key {
                        LayoutKey::Descriptor { content, .. } => content,
                        _ => &[],
                    })
                )
            })
            .word()
    })
}
/// True while the births pre-pass collects descriptor inputs (bodies are
/// not lowered, so lowering-driven use sets are empty).
pub(crate) fn collecting() -> bool {
    MODULE.with(|m| m.borrow().collecting)
}
/// An image build of this module: the whole-program ordinal plan is installed
/// and this is not the births pre-pass. Compiled state of such a build that
/// agents share may hold only static words.
pub(crate) fn image_plan_active() -> bool {
    MODULE.with(|m| {
        let m = m.borrow();
        !m.collecting && !m.plan.entries.is_empty()
    })
}
pub(crate) fn current_plan() -> std::sync::Arc<ProgramLayoutOrdinals> {
    MODULE.with(|m| m.borrow().plan.clone())
}

pub(crate) fn static_word(identity: u32) -> u32 {
    if MODULE.with(|m| m.borrow().collecting) {
        return 0;
    }
    current_plan()
        .identity_word(identity)
        .expect("compiler identity in whole-image ordinal plan")
}
pub(crate) fn request_symbol(identity: u32) -> String {
    format!("perry_layout_request_{identity}")
}
pub(crate) fn emit_request(module: &mut crate::module::LlModule, id: u32) {
    let plan = current_plan();
    let Some(word) = plan.identity_word(id) else {
        return;
    };
    if !MODULE.with(|m| m.borrow_mut().emitted_requests.insert(id)) {
        return;
    }
    let symbol = request_symbol(id);
    module.add_coalesced_global(
        &symbol,
        format!("@{symbol} = weak_odr global {{ i32, i32 }} {{ i32 {id}, i32 {word} }}, align 4"),
    );
}
pub(crate) fn emit_requests(
    module: &mut crate::module::LlModule,
    identities: &[(crate::BirthShape, u32)],
) {
    for (_, id) in identities {
        emit_request(module, *id);
    }
}

pub(crate) fn deferred_word(birth: &crate::BirthShape, live: u32) -> (u32, u32) {
    let mut content = birth.clone();
    content.keys.clear();
    content.key_count = 0;
    content.live = live;
    content.rep = 0;
    content.typed = None;
    let plan = current_plan();
    let entry = plan
        .entries
        .iter()
        .find(|e| {
            matches!(&e.key,
        LayoutKey::Identity(_, shape) if *shape == content)
        })
        .expect("deferred width has a static ordinal");
    (entry.requested_shape_id, entry.word())
}

#[cfg(test)]
mod template_word_tests {
    use super::*;

    fn descriptor(module: &str, symbol: &str) -> OrdinalInput {
        OrdinalInput {
            key: LayoutKey::Descriptor {
                module: module.into(),
                symbol: symbol.into(),
                content: vec![1, 2, 3],
            },
            requested_shape_id: 0,
            band: 0,
            proto_kind: 0,
            role: LayoutRole::ClassFamily,
        }
    }

    #[test]
    fn only_a_unique_compiler_descriptor_supplies_a_template() {
        let mut plan = ProgramLayoutOrdinals::assign(
            [],
            [
                descriptor("a", "unique"),
                descriptor("a", "ambiguous"),
                descriptor("b", "ambiguous"),
            ],
        )
        .unwrap();
        let mut metadata = plan.entries[0].clone();
        metadata.key = LayoutKey::Descriptor {
            module: "a".into(),
            symbol: "metadata".into(),
            content: vec![],
        };
        metadata.role = LayoutRole::OtherStatic;
        plan.entries.push(metadata);
        assert_eq!(plan.descriptor_template_word("metadata"), None);
        let entry = plan
            .entries
            .iter()
            .find(|e| match &e.key {
                LayoutKey::Descriptor { symbol, .. } => symbol == "unique",
                _ => false,
            })
            .unwrap();
        assert_eq!(plan.descriptor_template_word("unique"), Some(entry.word()));
        assert_eq!(plan.descriptor_template_word("ambiguous"), None);
        assert_eq!(plan.descriptor_template_word("opaque"), None);
    }
}
