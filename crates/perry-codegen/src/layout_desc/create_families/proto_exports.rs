//! The program-wide imported-prototype registry: the export facts every module's
//! prototype images may read, installed by the driver before module codegen.
//! Owns its own state (`PROGRAM_PROTO_EXPORTS`), so nothing outside this module
//! locks or replaces it directly.
use super::*;

static PROGRAM_PROTO_EXPORTS: RwLock<Option<Arc<ProtoExports>>> = RwLock::new(None);

/// Install the program's export facts (driver, before module codegen).
pub fn set_program_proto_exports(facts: Option<ProtoExports>) {
    *PROGRAM_PROTO_EXPORTS.write().unwrap() = facts.map(Arc::new);
}

pub(super) fn program_exports() -> Arc<ProtoExports> {
    PROGRAM_PROTO_EXPORTS
        .read()
        .unwrap()
        .clone()
        .unwrap_or_default()
}

/// The object-cache key of the installed export facts: every module's
/// prototype images may read them.
pub fn program_proto_exports_key() -> String {
    use std::hash::{Hash, Hasher};
    let facts = program_exports();
    if facts.is_empty() {
        return String::new();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    facts.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The export facts of every module (`modules`: path and HIR), to a fixed
/// point: a module's facts may read another's (a literal spreading an
/// imported one, a key imported from a third module).
pub fn program_proto_exports<'a>(modules: &[(String, &'a Module)]) -> ProtoExports {
    let mut facts = ProtoExports::new();
    for _ in 0..8 {
        let mut next = ProtoExports::new();
        for (path, hir) in modules {
            let a = analyze(hir, &facts);
            for export in &hir.exports {
                let perry_hir::Export::Named { local, exported } = export else {
                    continue;
                };
                let Some(id) = a.top_level.get(local.as_str()) else {
                    continue;
                };
                let fact = match a.consts.get(id) {
                    Some(KeyConst::Str(s)) => Some(ExportFact::Key(ImageKey::Str(s.clone()))),
                    Some(KeyConst::Symbol(Some(k))) if exportable(k) => {
                        Some(ExportFact::Key(k.clone()))
                    }
                    Some(KeyConst::Symbol(_)) => None,
                    None => a
                        .borns
                        .get(id)
                        .and_then(|b| b.image.clone())
                        .filter(|img| img.iter().all(|e| exportable(&e.key)))
                        .map(ExportFact::Image),
                };
                if let Some(fact) = fact {
                    next.insert((path.clone(), exported.clone()), fact);
                }
            }
        }
        if next == facts {
            break;
        }
        facts = next;
    }
    facts
}
