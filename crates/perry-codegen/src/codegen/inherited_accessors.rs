//! Receiver capabilities for inherited accessors, kept in the existing method
//! proof map. The declaration remains on its original prototype and body home.
use super::opts::CrossModuleCtx;
use perry_hir::{Class, Module};
use std::collections::{BTreeSet, HashMap};

pub(crate) fn receiver_body_name(public: &str, cid: u32) -> String {
    format!("{public}$receiver{cid}")
}

pub(super) fn register(
    hir: &Module,
    classes: &HashMap<String, &Class>,
    ids: &HashMap<String, u32>,
    methods: &mut HashMap<(String, String), String>,
    cross: &mut CrossModuleCtx,
) {
    let mut properties = BTreeSet::new();
    for c in &hir.classes {
        properties.extend(c.getters.iter().map(|(p, _)| (false, p.clone())));
        properties.extend(c.setters.iter().map(|(p, _)| (true, p.clone())));
    }
    for receiver in &hir.classes {
        for (setter, property) in &properties {
            let Some((owner_name, _)) = super::static_prototype::accessor_owner(
                classes,
                ids,
                &receiver.name,
                property,
                *setter,
            ) else {
                continue;
            };
            let owner = classes[&owner_name];
            if owner.name == receiver.name || owner.is_imported_stub() {
                continue;
            }
            let key_name = format!("__{}_{property}", if *setter { "set" } else { "get" });
            let key = (receiver.name.clone(), key_name.clone());
            if methods.contains_key(&key) {
                continue;
            }
            let accessors = if *setter {
                &owner.setters
            } else {
                &owner.getters
            };
            let Some((_, method)) = accessors.iter().find(|(p, _)| p == property) else {
                continue;
            };
            let mut method = method.clone();
            method.name = key_name.clone();
            let Some(fact) = crate::collectors::inherited_accessor_proven_this(
                receiver,
                owner,
                &method,
                classes,
                &cross.module_dispatch,
            ) else {
                continue;
            };
            let Some(public) = methods.get(&(owner_name.clone(), key_name)).cloned() else {
                continue;
            };
            // The inherited registry alias still names the ordinary public body.
            // Only a shape-authorized accessor call selects its receiver body.
            methods.insert(key.clone(), public);
            cross.pshape_methods.insert(key, fact);
        }
    }
}
