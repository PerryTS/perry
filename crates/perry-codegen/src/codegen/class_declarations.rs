//! Each class's ClassBody instance members as ONE constant in the image
//! (`perry-runtime`'s `object::class_registry::declarations`): method bodies
//! and closure-convention entries, accessor halves, names and source order.
//! Module init registers the constant's address once per class
//! (`js_register_class_declaration`); the runtime keeps no per-member table.
//!
//! A computed member (`[k]() {}`) is listed in source order without a name;
//! its class definition names it when it evaluates the key
//! (`js_register_class_computed_method` / `_accessor`, keyed by the member's
//! source-order token, its function id).

use crate::strings::StringPool;

use super::helpers::sanitize_member;
use super::spec_function_length;

/// The LLVM type of one runtime `ClassMemberDecl`.
const MEMBER_TYPE: &str = "{ ptr, ptr, ptr, i32, i32, i32, i8, i8, i16 }";
const KIND_METHOD: u8 = 0;
const KIND_GETTER: u8 = 1;
const KIND_SETTER: u8 = 2;
const FLAG_SYNTHETIC_ARGUMENTS: u8 = 1;
const FLAG_REST: u8 = 2;

/// One class's declaration constant: the global definitions to add to the
/// module and the symbol to register for `cid`.
pub(super) struct ClassDeclarationGlobal {
    pub(super) cid: u32,
    pub(super) symbol: String,
    pub(super) globals: [String; 2],
}

struct Member {
    order: u32,
    /// `ptr @bytes` or `ptr null`.
    name: String,
    name_len: usize,
    code: String,
    entry: String,
    param_count: usize,
    kind: u8,
    flags: u8,
    set_length: i32,
}

/// The declaration constants of `local_classes` (classes this module
/// defines, keyed by their ClassId). `info_ref` names (and requests) the
/// `JsFunctionInfo` global of a body. A class with no instance member and no
/// birth shape gets none.
pub(super) fn class_declaration_globals(
    local_classes: &[(u32, &perry_hir::Class)],
    strings: &StringPool,
    module_prefix: &str,
    info_ref: &mut dyn FnMut(&str) -> String,
) -> Vec<ClassDeclarationGlobal> {
    let mut out = Vec::new();
    for &(cid, class) in local_classes {
        let class_symbol = sanitize_member(&class.name);
        let method_symbol = |name: &str| {
            format!(
                "perry_method_{}__{}__{}",
                module_prefix,
                class_symbol,
                sanitize_member(name)
            )
        };
        let named = |name: &str| {
            strings
                .lookup(name)
                .map(|e| (format!("ptr @{}", e.bytes_global), e.byte_len))
        };
        let mut members: Vec<Member> = Vec::new();
        for method in &class.methods {
            let Some((name, name_len)) = named(&method.name) else {
                continue;
            };
            let code = method_symbol(&method.name);
            let synthetic = method
                .params
                .last()
                .is_some_and(|p| p.arguments_object.is_some());
            // A method reading `arguments` after `...rest` has two trailing
            // array parameters; the user rest is any rest that is not the
            // synthesized `arguments` slot.
            let rest = method
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            members.push(Member {
                order: method.id,
                name,
                name_len,
                entry: format!("ptr {}", info_ref(&format!("{code}__eclo"))),
                code: format!("ptr @{code}"),
                param_count: method.params.len(),
                kind: KIND_METHOD,
                flags: if synthetic {
                    FLAG_SYNTHETIC_ARGUMENTS
                } else {
                    0
                } | if rest { FLAG_REST } else { 0 },
                set_length: -1,
            });
        }
        for (prop, getter) in &class.getters {
            if class.static_accessor_fn_ids.contains(&getter.id) {
                continue;
            }
            let Some((name, name_len)) = named(prop) else {
                continue;
            };
            members.push(Member {
                order: getter.id,
                name,
                name_len,
                code: format!("ptr @{}", method_symbol(&format!("__get_{}", getter.name))),
                entry: "ptr null".to_string(),
                param_count: 0,
                kind: KIND_GETTER,
                flags: 0,
                set_length: -1,
            });
        }
        for (prop, setter) in &class.setters {
            if class.static_accessor_fn_ids.contains(&setter.id) {
                continue;
            }
            let Some((name, name_len)) = named(prop) else {
                continue;
            };
            members.push(Member {
                order: setter.id,
                name,
                name_len,
                code: format!("ptr @{}", method_symbol(&format!("__set_{}", setter.name))),
                entry: "ptr null".to_string(),
                param_count: 1,
                kind: KIND_SETTER,
                flags: 0,
                set_length: spec_function_length(&setter.params) as i32,
            });
        }
        for member in class.computed_members.iter().filter(|m| !m.is_static) {
            let f = &member.function;
            let kind = match member.kind {
                perry_hir::ClassComputedMemberKind::Method => KIND_METHOD,
                perry_hir::ClassComputedMemberKind::Getter => KIND_GETTER,
                perry_hir::ClassComputedMemberKind::Setter => KIND_SETTER,
            };
            members.push(Member {
                order: f.id,
                name: "ptr null".to_string(),
                name_len: 0,
                code: format!("ptr @{}", method_symbol(&f.name)),
                entry: "ptr null".to_string(),
                param_count: f.params.len(),
                kind,
                flags: if kind == KIND_METHOD && f.params.last().is_some_and(|p| p.is_rest) {
                    FLAG_REST
                } else {
                    0
                },
                set_length: -1,
            });
        }
        let birth_shape =
            super::static_shape_ids::static_prototype_shape(cid).map_or(0, |(id, _)| id);
        if members.is_empty() && birth_shape == 0 {
            continue;
        }
        members.sort_by_key(|m| m.order);
        let members_symbol = format!("__perry_class_decl_members_{cid}");
        let symbol = format!("__perry_class_decl_{cid}");
        let body = members
            .iter()
            .map(|m| {
                format!(
                    "{MEMBER_TYPE} {{ {}, {}, {}, i32 {}, i32 {}, i32 {}, i8 {}, i8 {}, i16 {} }}",
                    m.name,
                    m.code,
                    m.entry,
                    m.name_len,
                    m.order,
                    m.param_count,
                    m.kind,
                    m.flags,
                    m.set_length,
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let members_global = if members.is_empty() {
            format!("@{members_symbol} = private unnamed_addr constant [0 x {MEMBER_TYPE}] zeroinitializer")
        } else {
            format!(
                "@{members_symbol} = private unnamed_addr constant [{} x {MEMBER_TYPE}] [{body}]",
                members.len()
            )
        };
        // Summary flags (runtime `CLASS_DECL_HAS_ACCESSORS` = 1,
        // `CLASS_DECL_HAS_COMPUTED` = 2).
        let flags = u32::from(members.iter().any(|m| m.kind != KIND_METHOD))
            | if members
                .iter()
                .any(|m| m.name_len == 0 && m.name == "ptr null")
            {
                2
            } else {
                0
            };
        let decl_global = format!(
            "@{symbol} = private unnamed_addr constant {{ ptr, i32, i32, i32, i32 }} {{ ptr @{members_symbol}, i32 {}, i32 {birth_shape}, i32 {flags}, i32 0 }}",
            members.len()
        );
        out.push(ClassDeclarationGlobal {
            cid,
            symbol,
            globals: [members_global, decl_global],
        });
    }
    out
}
