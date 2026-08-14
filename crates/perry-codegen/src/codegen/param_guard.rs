//! Runtime type descriptors for guarded ordinary-parameter specialization.
//!
//! TypeScript annotations are candidates, never proofs. This module turns the
//! guardable subset into a compact, immutable graph consumed by
//! `js_param_type_guard`. Graph edges (rather than recursively nested bytes)
//! let recursive aliases such as `Node` and `Env` terminate, and deterministic
//! field ordering keeps object-cache inputs stable.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;

#[derive(Debug, Clone)]
pub(crate) struct SpecParamGuard {
    /// The exact HIR fact made available only inside the successful clone.
    pub proof: Type,
    /// Module-unique rodata symbol containing `descriptor`.
    pub descriptor_name: String,
    pub descriptor: Vec<u8>,
}

#[derive(Debug, Clone)]
struct GuardField {
    name: String,
    optional: bool,
    ty: u32,
}

#[derive(Debug, Clone)]
enum GuardNode {
    Any,
    Number,
    Int32,
    Boolean,
    String,
    StringLiteral(String),
    Null,
    Undefined,
    BigInt,
    Symbol,
    Array(u32),
    Tuple(Vec<u32>),
    Object {
        class_id: Option<u32>,
        fields: Vec<GuardField>,
    },
    Union(Vec<u32>),
    RecursiveRef(u32),
    Map {
        key: u32,
        value: u32,
    },
    Set(u32),
}

struct GuardGraphBuilder<'a> {
    nodes: Vec<GuardNode>,
    named: HashMap<String, u32>,
    building_named: HashSet<String>,
    type_aliases: &'a HashMap<String, Type>,
    interfaces: &'a HashMap<String, perry_hir::Interface>,
    classes: &'a HashMap<String, &'a perry_hir::Class>,
    class_ids: &'a HashMap<String, u32>,
}

impl<'a> GuardGraphBuilder<'a> {
    fn reserve(&mut self) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(GuardNode::Any);
        id
    }

    fn push(&mut self, node: GuardNode) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        id
    }

    fn build_fields(
        &mut self,
        fields: impl IntoIterator<Item = (String, Type, bool)>,
    ) -> Option<Vec<GuardField>> {
        fields
            .into_iter()
            .map(|(name, ty, optional)| {
                // The proof consumers currently expose a property's declared
                // type, not `T | undefined`. Do not admit optional fields
                // until that fact propagation models absence explicitly.
                if optional {
                    return None;
                }
                Some(GuardField {
                    name,
                    optional,
                    ty: self.build_type(&ty, true)?,
                })
            })
            .collect()
    }

    fn build_named(&mut self, name: &str) -> Option<u32> {
        if let Some(id) = self.named.get(name) {
            return if self.building_named.contains(name) {
                Some(self.push(GuardNode::RecursiveRef(*id)))
            } else {
                Some(*id)
            };
        }
        let id = self.reserve();
        self.named.insert(name.to_string(), id);
        self.building_named.insert(name.to_string());

        let node = if let Some(alias) = self.type_aliases.get(name) {
            let alias_id = self.build_type(alias, true)?;
            self.nodes.get(alias_id as usize)?.clone()
        } else if let Some(interface) = self.interfaces.get(name) {
            // Extended/generic interfaces need substitution + inherited-field
            // flattening. Stay generic until the descriptor can prove both.
            if !interface.extends.is_empty()
                || !interface.type_params.is_empty()
                || !interface.methods.is_empty()
            {
                return None;
            }
            let fields = self.build_fields(
                interface
                    .properties
                    .iter()
                    .map(|p| (p.name.clone(), p.ty.clone(), p.optional)),
            )?;
            GuardNode::Object {
                class_id: None,
                fields,
            }
        } else if self.classes.contains_key(name) && self.class_ids.contains_key(name) {
            // Class identity alone cannot prove mutable field values, while
            // compact instances do not expose the ordinary `keys_array`
            // needed for read-only field validation. Keep class parameters on
            // the generic path until a layout-aware field guard exists.
            return None;
        } else {
            self.building_named.remove(name);
            self.named.remove(name);
            self.nodes.pop();
            return None;
        };
        self.building_named.remove(name);
        self.nodes[id as usize] = node;
        Some(id)
    }

    fn build_type(&mut self, ty: &Type, nested: bool) -> Option<u32> {
        Some(match ty {
            Type::Any | Type::Unknown | Type::TypeVar(_) if nested => self.push(GuardNode::Any),
            Type::Any | Type::Unknown | Type::TypeVar(_) | Type::Never => return None,
            Type::Void => self.push(GuardNode::Undefined),
            Type::Null => self.push(GuardNode::Null),
            Type::Boolean => self.push(GuardNode::Boolean),
            Type::Number => self.push(GuardNode::Number),
            Type::Int32 => self.push(GuardNode::Int32),
            Type::BigInt => self.push(GuardNode::BigInt),
            Type::String => self.push(GuardNode::String),
            Type::StringLiteral(value) => self.push(GuardNode::StringLiteral(value.clone())),
            Type::Symbol => self.push(GuardNode::Symbol),
            Type::Array(elem) => {
                let elem = self.build_type(elem, true)?;
                self.push(GuardNode::Array(elem))
            }
            Type::Tuple(elems) => {
                let elems = elems
                    .iter()
                    .map(|elem| self.build_type(elem, true))
                    .collect::<Option<Vec<_>>>()?;
                self.push(GuardNode::Tuple(elems))
            }
            Type::Object(obj) => {
                // A finite field descriptor does not prove arbitrary values
                // reachable through an index signature.
                if obj.index_signature.is_some() {
                    return None;
                }
                let mut names = obj
                    .property_order
                    .clone()
                    .unwrap_or_else(|| obj.properties.keys().cloned().collect());
                if obj.property_order.is_none() {
                    names.sort();
                }
                let fields = self.build_fields(names.into_iter().filter_map(|name| {
                    obj.properties
                        .get(&name)
                        .map(|p| (name, p.ty.clone(), p.optional))
                }))?;
                self.push(GuardNode::Object {
                    class_id: None,
                    fields,
                })
            }
            Type::Union(variants) => {
                if variants.is_empty() {
                    return None;
                }
                let variants = variants
                    .iter()
                    .map(|variant| self.build_type(variant, true))
                    .collect::<Option<Vec<_>>>()?;
                self.push(GuardNode::Union(variants))
            }
            Type::Named(name) => self.build_named(name)?,
            Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
                let elem = self.build_type(&type_args[0], true)?;
                self.push(GuardNode::Array(elem))
            }
            Type::Generic { base, type_args } if base == "Map" && type_args.len() == 2 => {
                let key = self.build_type(&type_args[0], true)?;
                let value = self.build_type(&type_args[1], true)?;
                self.push(GuardNode::Map { key, value })
            }
            Type::Generic { base, type_args } if base == "Set" && type_args.len() == 1 => {
                let elem = self.build_type(&type_args[0], true)?;
                self.push(GuardNode::Set(elem))
            }
            Type::Generic { .. } | Type::Promise(_) | Type::Function(_) => return None,
        })
    }
}

const MAGIC: u32 = 0x3154_4750; // `PGT1`, little-endian.

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn encode_node(node: &GuardNode) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match node {
        GuardNode::Any => out.push(0),
        GuardNode::Number => out.push(1),
        GuardNode::Int32 => out.push(2),
        GuardNode::Boolean => out.push(3),
        GuardNode::String => out.push(4),
        GuardNode::Null => out.push(5),
        GuardNode::Undefined => out.push(6),
        GuardNode::BigInt => out.push(7),
        GuardNode::Symbol => out.push(8),
        GuardNode::Array(elem) => {
            out.push(9);
            put_u32(&mut out, *elem);
        }
        GuardNode::Tuple(elems) => {
            out.push(10);
            put_u32(&mut out, elems.len().try_into().ok()?);
            for elem in elems {
                put_u32(&mut out, *elem);
            }
        }
        GuardNode::Object { class_id, fields } => {
            out.push(11);
            put_u32(&mut out, class_id.unwrap_or(0));
            put_u32(&mut out, fields.len().try_into().ok()?);
            for field in fields {
                out.push(field.optional as u8);
                put_u16(&mut out, field.name.len().try_into().ok()?);
                out.extend_from_slice(field.name.as_bytes());
                put_u32(&mut out, field.ty);
            }
        }
        GuardNode::Union(variants) => {
            out.push(12);
            put_u32(&mut out, variants.len().try_into().ok()?);
            for variant in variants {
                put_u32(&mut out, *variant);
            }
        }
        GuardNode::StringLiteral(value) => {
            out.push(13);
            put_u32(&mut out, value.len().try_into().ok()?);
            out.extend_from_slice(value.as_bytes());
        }
        GuardNode::RecursiveRef(target) => {
            out.push(14);
            put_u32(&mut out, *target);
        }
        GuardNode::Map { key, value } => {
            out.push(15);
            put_u32(&mut out, *key);
            put_u32(&mut out, *value);
        }
        GuardNode::Set(elem) => {
            out.push(16);
            put_u32(&mut out, *elem);
        }
    }
    Some(out)
}

fn descriptor_for_type(
    ty: &Type,
    type_aliases: &HashMap<String, Type>,
    interfaces: &HashMap<String, perry_hir::Interface>,
    classes: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
) -> Option<Vec<u8>> {
    let mut builder = GuardGraphBuilder {
        nodes: Vec::new(),
        named: HashMap::new(),
        building_named: HashSet::new(),
        type_aliases,
        interfaces,
        classes,
        class_ids,
    };
    let root = builder.build_type(ty, false)?;
    let bodies = builder
        .nodes
        .iter()
        .map(encode_node)
        .collect::<Option<Vec<_>>>()?;
    let node_count: u32 = bodies.len().try_into().ok()?;
    let header_len = 12usize.checked_add((bodies.len() + 1).checked_mul(4)?)?;
    let mut offset: u32 = header_len.try_into().ok()?;
    let mut out = Vec::with_capacity(header_len + bodies.iter().map(Vec::len).sum::<usize>());
    put_u32(&mut out, MAGIC);
    put_u32(&mut out, root);
    put_u32(&mut out, node_count);
    for body in &bodies {
        put_u32(&mut out, offset);
        offset = offset.checked_add(body.len().try_into().ok()?)?;
    }
    put_u32(&mut out, offset);
    for body in bodies {
        out.extend_from_slice(&body);
    }
    Some(out)
}

pub(crate) fn declaration_guards(
    function_id: u32,
    module_prefix: &str,
    params: &[perry_hir::Param],
    demoted_params: &[bool],
    type_aliases: &HashMap<String, Type>,
    interfaces: &HashMap<String, perry_hir::Interface>,
    classes: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
) -> Vec<Option<SpecParamGuard>> {
    params
        .iter()
        .zip(demoted_params.iter())
        .enumerate()
        .map(|(index, (param, demoted))| {
            if *demoted || matches!(param.ty, Type::Any | Type::Unknown | Type::Never) {
                return None;
            }
            Some(SpecParamGuard {
                proof: param.ty.clone(),
                descriptor_name: format!(
                    "perry_param_guard_{}_{}_{}",
                    module_prefix, function_id, index
                ),
                descriptor: descriptor_for_type(
                    &param.ty,
                    type_aliases,
                    interfaces,
                    classes,
                    class_ids,
                )?,
            })
        })
        .collect()
}

/// Whether the current function body can suspend after its entry guard.
/// `walk_expr_children` intentionally does not enter nested closure bodies;
/// those execute under their own entry contracts and must not disqualify the
/// enclosing function.
pub(crate) fn body_contains_await(stmts: &[perry_hir::Stmt]) -> bool {
    fn expr_contains_await(expr: &perry_hir::Expr) -> bool {
        if matches!(expr, perry_hir::Expr::Await(_)) {
            return true;
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            found |= expr_contains_await(child);
        });
        found
    }

    stmts.iter().any(|stmt| match stmt {
        perry_hir::Stmt::Expr(expr) | perry_hir::Stmt::Throw(expr) => expr_contains_await(expr),
        perry_hir::Stmt::Return(Some(expr)) => expr_contains_await(expr),
        perry_hir::Stmt::Let {
            init: Some(expr), ..
        } => expr_contains_await(expr),
        perry_hir::Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_contains_await(condition)
                || body_contains_await(then_branch)
                || else_branch.as_deref().is_some_and(body_contains_await)
        }
        perry_hir::Stmt::While { condition, body }
        | perry_hir::Stmt::DoWhile { condition, body } => {
            expr_contains_await(condition) || body_contains_await(body)
        }
        perry_hir::Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref()
                .is_some_and(|stmt| body_contains_await(std::slice::from_ref(stmt)))
                || condition.as_ref().is_some_and(expr_contains_await)
                || update.as_ref().is_some_and(expr_contains_await)
                || body_contains_await(body)
        }
        perry_hir::Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body_contains_await(body)
                || catch
                    .as_ref()
                    .is_some_and(|catch| body_contains_await(&catch.body))
                || finally.as_deref().is_some_and(body_contains_await)
        }
        perry_hir::Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_await(discriminant)
                || cases.iter().any(|case| {
                    case.test.as_ref().is_some_and(expr_contains_await)
                        || body_contains_await(&case.body)
                })
        }
        perry_hir::Stmt::Labeled { body, .. } => {
            body_contains_await(std::slice::from_ref(body.as_ref()))
        }
        _ => false,
    })
}

/// LLVM `c"..."` encoding for a binary descriptor plus its sentinel byte.
pub(crate) fn descriptor_llvm_literal(bytes: &[u8]) -> String {
    let mut out = String::from("c\"");
    for byte in bytes.iter().copied().chain(std::iter::once(0)) {
        if (32..127).contains(&byte) && byte != b'"' && byte != b'\\' {
            out.push(byte as char);
        } else {
            out.push('\\');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_alias_serializes_as_a_finite_graph() {
        let mut props = HashMap::new();
        props.insert(
            "next".to_string(),
            perry_hir::types::PropertyInfo {
                ty: Type::Union(vec![Type::Named("Node".to_string()), Type::Null]),
                optional: false,
                readonly: false,
            },
        );
        let aliases = HashMap::from([(
            "Node".to_string(),
            Type::Object(perry_hir::types::ObjectType {
                name: Some("Node".to_string()),
                properties: props,
                property_order: Some(vec!["next".to_string()]),
                index_signature: None,
            }),
        )]);
        let descriptor = descriptor_for_type(
            &Type::Named("Node".to_string()),
            &aliases,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(descriptor[0..4].try_into().unwrap()),
            MAGIC
        );
        assert!(
            descriptor.len() < 128,
            "recursive graph unexpectedly expanded"
        );
        assert!(
            descriptor.iter().any(|byte| *byte == 14),
            "recursive aliases must close with a finite graph edge"
        );
    }

    #[test]
    fn suspension_scan_stays_in_the_current_function_body() {
        let direct = vec![perry_hir::Stmt::Return(Some(perry_hir::Expr::Await(
            Box::new(perry_hir::Expr::Undefined),
        )))];
        assert!(body_contains_await(&direct));

        let nested = vec![perry_hir::Stmt::Expr(perry_hir::Expr::Closure {
            func_id: 9,
            params: Vec::new(),
            return_type: Type::Void,
            body: direct,
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_async: true,
            is_generator: false,
            is_arrow: true,
            is_strict: true,
        })];
        assert!(!body_contains_await(&nested));
    }

    #[test]
    fn collection_generics_serialize_their_complete_element_types() {
        let descriptor = descriptor_for_type(
            &Type::Generic {
                base: "Map".to_string(),
                type_args: vec![Type::String, Type::Number],
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();
        assert!(descriptor.iter().any(|byte| *byte == 15));

        let descriptor = descriptor_for_type(
            &Type::Generic {
                base: "Set".to_string(),
                type_args: vec![Type::Boolean],
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();
        assert!(descriptor.iter().any(|byte| *byte == 16));
    }
}
