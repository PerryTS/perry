use crate::ir::{Enum, EnumMember, EnumValue, Module};
use crate::lower::LoweringContext;

/// TypeScript is intentionally a strict partial compatibility package (#8511),
/// so apply the public-named-export gate used by Node core. The caller defers
/// the error until a value read because mixed imports can contain erased types.
pub(super) fn should_defer_unknown_named_import(
    source: &str,
    imported: &str,
    is_node_core: bool,
) -> bool {
    (is_node_core || source == "typescript")
        && !perry_api_manifest::module_has_public_named_export(source, imported)
}

/// Register TypeScript's runtime enums as HIR enums so OpenCode imports such as
/// `ScriptTarget.ESNext` fold without embedding the upstream compiler object.
pub(super) fn register_runtime_enum(
    ctx: &mut LoweringContext,
    module: &mut Module,
    source: &str,
    local: &str,
    imported: &str,
) {
    if source != "typescript" {
        return;
    }
    let Some(runtime_members) = runtime_enum_members(imported) else {
        return;
    };
    let enum_id = ctx.fresh_enum();
    let members: Vec<EnumMember> = runtime_members
        .iter()
        .map(|(name, value)| EnumMember {
            name: (*name).to_string(),
            value: EnumValue::Number(*value),
        })
        .collect();
    ctx.define_enum(
        local.to_string(),
        enum_id,
        members
            .iter()
            .map(|member| (member.name.clone(), member.value.clone()))
            .collect(),
    );
    module.enums.push(Enum {
        id: enum_id,
        name: local.to_string(),
        members,
        is_exported: false,
    });
}

fn runtime_enum_members(name: &str) -> Option<&'static [(&'static str, i64)]> {
    match name {
        "ScriptTarget" => Some(&[
            ("ES3", 0),
            ("ES5", 1),
            ("ES2015", 2),
            ("ES2016", 3),
            ("ES2017", 4),
            ("ES2018", 5),
            ("ES2019", 6),
            ("ES2020", 7),
            ("ES2021", 8),
            ("ES2022", 9),
            ("ES2023", 10),
            ("ES2024", 11),
            ("ESNext", 99),
            ("Latest", 99),
            ("JSON", 100),
        ]),
        "ModuleKind" => Some(&[
            ("None", 0),
            ("CommonJS", 1),
            ("AMD", 2),
            ("UMD", 3),
            ("System", 4),
            ("ES2015", 5),
            ("ES2020", 6),
            ("ES2022", 7),
            ("ESNext", 99),
            ("Node16", 100),
            ("Node18", 101),
            ("Node20", 102),
            ("NodeNext", 199),
            ("Preserve", 200),
        ]),
        "DiagnosticCategory" => Some(&[
            ("Warning", 0),
            ("Error", 1),
            ("Suggestion", 2),
            ("Message", 3),
        ]),
        _ => None,
    }
}
