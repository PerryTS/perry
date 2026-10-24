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
    pub family_first: u32,
    pub family_count: u32,
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
        let family_first = entries.len() as u32;
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
        let family = |i: &&OrdinalInput| {
            i.role == LayoutRole::ClassFamily && i.band == 0 && i.proto_kind == 0
        };
        for input in sorted.values().filter(family) {
            Self::append(&mut entries, input.clone())?;
        }
        let family_count = entries.len() as u32 - family_first;
        for input in sorted.values().filter(|i| !family(i)) {
            Self::append(&mut entries, input.clone())?;
        }
        Ok(Self {
            entries,
            family_first,
            family_count,
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

    /// Exact stable cache input, including full keys (not just content hashes),
    /// header bounds, ABI version and the entire native catalog.
    pub fn cache_key(&self) -> String {
        format!("layout-word-v1:{self:?}")
    }

    pub fn image_initializer(&self) -> String {
        format!(
            "{{ i32 1, i32 {}, i32 {}, i32 {} }}",
            self.static_rows(),
            self.family_first,
            self.family_count
        )
    }
}

impl OrdinalEntry {
    pub fn word(&self) -> u32 {
        encode_layout_word(self.ordinal, self.band, self.proto_kind)
            .expect("validated ordinal entry")
    }
}

/// Checked compiler encoding. These ABI constants are consumed from perry-abi
/// once the runtime lane's first ABI commit is available.
pub fn encode_layout_word(row: u32, band: u8, proto_kind: u8) -> Result<u32, String> {
    if row >= (1 << 26) || band >= 3 || proto_kind >= 4 {
        return Err("invalid LayoutWord row, band or prototype kind".into());
    }
    Ok(0x8000_0000 | (u32::from(band) << 28) | (u32::from(proto_kind) << 26) | row)
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
