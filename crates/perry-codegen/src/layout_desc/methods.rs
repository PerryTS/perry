//! Static family method tables, derived only from complete prototype shapes.
use super::*;
use std::collections::BTreeSet;

pub(super) fn emit(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    atoms: &mut BTreeMap<String, String>,
    fam: &ClassFamilyLayout,
) -> String {
    let mut cid = fam.class_id;
    let mut guards = Vec::new();
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut methods = Vec::new();
    for _ in 0..64 {
        if !seen.insert(cid) {
            break;
        }
        let Some((word, shape)) = crate::codegen::static_shape_ids::static_prototype_layout(cid)
        else {
            break;
        };
        guards.push((cid, word));
        for (slot, bytes) in shape
            .keys
            .split(|b| *b == 0)
            .take(shape.key_count as usize)
            .enumerate()
        {
            let Ok(name) = std::str::from_utf8(bytes) else {
                continue;
            };
            if !names.insert(name.to_string())
                || fam.keys.iter().any(|(k, _)| k == name)
                || name == "constructor"
                || !shape.constfn.iter().any(|f| f.slot as usize == slot)
            {
                continue;
            }
            let Some(a) = atom(llmod, strings, atoms, name) else {
                continue;
            };
            methods.push((name.to_string(), a, guards.clone(), slot));
        }
        match shape.proto {
            crate::codegen::BirthProto::Prototype(_, parent) if parent != 0 => cid = parent,
            _ => break,
        }
    }
    if methods.is_empty() {
        return "null".into();
    }
    let cap = (methods.len() * 2).next_power_of_two();
    let mut entries = vec!["{ ptr, ptr, i32, i32 } zeroinitializer".to_string(); cap];
    for (i, (name, a, chain, slot)) in methods.iter().enumerate() {
        let g = format!("perry_family_method_guards_{}_{}", fam.class_id, i);
        let items = chain
            .iter()
            .map(|(cid, word)| format!("{{ i32, i32 }} {{ i32 {cid}, i32 {word} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        llmod.add_raw_global(format!(
            "@{g} = private constant [{} x {{ i32, i32 }}] [{items}], align 4",
            chain.len()
        ));
        let mut at = atom_hash(name) as usize & (cap - 1);
        while !entries[at].ends_with("zeroinitializer") {
            at = (at + 1) & (cap - 1);
        }
        entries[at] = format!(
            "{{ ptr, ptr, i32, i32 }} {{ ptr @{a}, ptr @{g}, i32 {}, i32 {slot} }}",
            chain.len()
        );
    }
    let name = format!("perry_family_methods_{}", fam.class_id);
    llmod.add_raw_global(format!(
        "@{name}_entries = private constant [{cap} x {{ ptr, ptr, i32, i32 }}] [{}], align 8",
        entries.join(", ")
    ));
    llmod.add_raw_global(format!("@{name} = private constant {{ i32, i32, ptr }} {{ i32 {}, i32 0, ptr @{name}_entries }}, align 8", cap - 1));
    format!("@{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{BirthProto, BirthShape, ConstFnBirth};
    use std::collections::HashMap;

    fn prototype(cid: u32, parent: u32, keys: &[(&str, bool)]) -> BirthShape {
        BirthShape {
            keys: keys
                .iter()
                .flat_map(|(k, _)| k.bytes().chain([0]))
                .collect(),
            key_count: keys.len() as u32,
            live: keys.len() as u32,
            proto: BirthProto::Prototype(cid, parent),
            typed: None,
            rep: 0,
            constfn: keys
                .iter()
                .enumerate()
                .filter(|(_, (_, method))| *method)
                .map(|(i, _)| ConstFnBirth {
                    slot: i as u8,
                    symbol: format!("body_{cid}_{i}$info"),
                })
                .collect(),
            private: vec![],
            brands: vec![],
            attrs: vec![0; keys.len()],
        }
    }
    fn table(own: &[&str], complete_leaf: bool) -> String {
        let mut shapes = vec![(
            prototype(
                700,
                0,
                &[("constructor", false), ("m", true), ("blocked", true)],
            ),
            100000,
        )];
        if complete_leaf {
            shapes.push((
                prototype(
                    701,
                    700,
                    &[("constructor", false), ("own", true), ("blocked", false)],
                ),
                100001,
            ));
        }
        let plan = crate::ProgramLayoutOrdinals::assign(
            crate::layout_ordinals::native_inputs(),
            shapes
                .iter()
                .map(|(s, id)| crate::layout_ordinals::identity_input(s, *id)),
        )
        .unwrap();
        crate::layout_ordinals::begin_module("test", std::sync::Arc::new(plan), false);
        crate::codegen::static_shape_ids::set_module_static_ids(
            "test",
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &shapes,
            &Default::default(),
        );
        let family = ClassFamilyLayout {
            class_id: 701,
            parent_class_id: 700,
            keys: own
                .iter()
                .enumerate()
                .map(|(i, k)| (k.to_string().into(), i as u32))
                .collect(),
            slot_count: own.len() as u32,
            own_count: own.len() as u32,
            init_count: own.len() as u32,
            discriminant: 0,
            variants: vec![],
        };
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let mut strings = StringPool::new();
        let pointer = emit(&mut module, &mut strings, &mut BTreeMap::new(), &family);
        let ir = module.to_ir();
        crate::codegen::static_shape_ids::set_module_static_ids(
            "test",
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &Default::default(),
        );
        format!("{pointer}\n{ir}")
    }
    #[test]
    fn family_method_index_keeps_complete_inherited_proofs_and_stops_at_accessors() {
        let ir = table(&[], true);
        assert!(ir.contains(&format!(
            "i32 701, i32 {}",
            crate::layout_ordinals::static_word(100001)
        )));
        assert!(ir.contains(&format!(
            "i32 700, i32 {}",
            crate::layout_ordinals::static_word(100000)
        )));
        assert!(
            ir.contains("i32 2, i32 1"),
            "inherited m needs both holder words"
        );
        assert!(
            !ir.contains("c\"blocked"),
            "an accessor blocks the parent's same-name method"
        );
        assert!(!ir.starts_with("null"));
    }
    #[test]
    fn own_layout_keys_and_incomplete_chains_cannot_bind_inherited_methods() {
        assert!(table(&[], false).starts_with("null"));
        let ir = table(&["m", "own"], true);
        assert!(
            ir.starts_with("null"),
            "own slots shadow both callable candidates"
        );
    }
}
