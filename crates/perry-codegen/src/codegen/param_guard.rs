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

    /// Every declared instance field on `name`'s inheritance chain, in
    /// root-to-leaf declaration order, with the most-derived declaration
    /// winning a shadowed name.
    ///
    /// Returns `None` — keeping the parameter generic — for any class whose
    /// declared field set is not the whole truth about its instances:
    ///
    /// * generic (unsubstituted `T`-typed fields),
    /// * a native, dynamic, or unresolvable base, whose fields HIR cannot see,
    /// * a computed-key field, whose `name` is a synthetic placeholder rather
    ///   than the runtime key,
    /// * a private field, which is not an ordinary own key,
    /// * an accessor anywhere on the chain that shares a field's name, since
    ///   the read the proof licenses would then run user code.
    ///
    /// Cycle-guarded like every other chain walk in this crate: same-named
    /// classes pulled across modules into one name-keyed table can form a
    /// parent cycle (`type_analysis_class_fields.rs` carries the same note).
    fn class_chain_fields(&mut self, name: &str) -> Option<Vec<GuardField>> {
        let mut chain: Vec<&perry_hir::Class> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut current = Some(name.to_string());
        while let Some(class_name) = current {
            if chain.len() > 64 || !seen.insert(class_name.clone()) {
                return None;
            }
            let class = self.classes.get(class_name.as_str()).copied()?;
            if !class.type_params.is_empty()
                || class.native_extends.is_some()
                || class.extends_expr.is_some()
            {
                return None;
            }
            chain.push(class);
            current = class.extends_name.clone();
        }
        // Root first, so a subclass's redeclaration overwrites its parent's.
        chain.reverse();
        let mut order: Vec<String> = Vec::new();
        let mut declared: HashMap<String, Type> = HashMap::new();
        let mut accessors: HashSet<&str> = HashSet::new();
        for class in &chain {
            for (accessor, _) in class.getters.iter().chain(class.setters.iter()) {
                accessors.insert(accessor.as_str());
            }
            for field in &class.fields {
                if field.key_expr.is_some() || field.is_private {
                    return None;
                }
                if declared
                    .insert(field.name.clone(), field.ty.clone())
                    .is_none()
                {
                    order.push(field.name.clone());
                }
            }
        }
        if order.iter().any(|field| accessors.contains(field.as_str())) {
            return None;
        }
        let fields: Vec<(String, Type, bool)> = order
            .into_iter()
            .map(|field| {
                let ty = declared.get(&field).cloned()?;
                Some((field, ty, false))
            })
            .collect::<Option<Vec<_>>>()?;
        self.build_fields(fields)
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
        } else if let Some(class_id) = self.class_ids.get(name).copied().filter(|id| *id != 0) {
            // (#8099) A class parameter is validated exactly like an interface
            // one — every declared field on the inheritance chain, by name —
            // plus a `class_chain_reaches` identity check that no structural
            // type can supply. The identity half is what gives
            // `param_type_guard.rs`'s class branch its first caller.
            //
            // The refusal this replaces claimed compact class instances have
            // no `keys_array` to validate against. They do:
            // `object_alloc_class_inline_keys_impl` installs a per-class array
            // built once at module init, so `own_data_field` resolves a class
            // instance's fields the same way it resolves a literal's. The
            // stale claim came from the doc comment on
            // `ObjectHeader::keys_array`, corrected alongside this.
            //
            // Identity ALONE was measured and rejected: with `fields` empty the
            // emitted clone comes out structurally identical to the `$generic`
            // sibling it routes around — same line count, same call multiset,
            // `js_typed_feedback_class_field_get_guard` already present in both
            // — so a class-annotated receiver reaches the class-field guard
            // path with no parameter evidence at all. It bought nothing and
            // cost one guard call per invocation: -51% on `tree`, -30% on
            // `tree_wide`. The field VALUE facts are the whole payload, which
            // is why they are not optional here.
            //
            // Cost is bounded by #8094's existing rule rather than a new one:
            // a field-bearing descriptor claims heap CONTENTS, so a
            // reference-typed parameter carrying one is refused in any body
            // that contains a call. A recursive class (`Tree.left: Tree`)
            // therefore cannot be guarded in the recursive walker that would
            // make its validation O(nodes x depth).
            let fields = self.class_chain_fields(name)?;
            GuardNode::Object {
                class_id: Some(class_id),
                fields,
            }
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
    // (#8094) Guard-only ineligibility, kept SEPARATE from `demoted_params`
    // because that mask also drives raw representation selection: a reference
    // parameter that cannot keep a descriptor proof can still be passed in a
    // raw slot.
    guard_blocked: &[bool],
    type_aliases: &HashMap<String, Type>,
    interfaces: &HashMap<String, perry_hir::Interface>,
    classes: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
) -> Vec<Option<SpecParamGuard>> {
    params
        .iter()
        .zip(demoted_params.iter())
        .zip(guard_blocked.iter())
        .enumerate()
        .map(|(index, ((param, demoted), blocked))| {
            if *demoted || *blocked || matches!(param.ty, Type::Any | Type::Unknown | Type::Never) {
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

    fn class(
        id: u32,
        name: &str,
        extends: Option<&str>,
        fields: Vec<(&str, Type)>,
    ) -> perry_hir::Class {
        perry_hir::Class {
            id,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: extends.map(str::to_string),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: fields
                .into_iter()
                .map(|(field, ty)| perry_hir::ClassField {
                    name: field.to_string(),
                    key_expr: None,
                    ty,
                    init: None,
                    is_private: false,
                    is_readonly: false,
                    decorators: Vec::new(),
                })
                .collect(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
            aliases: Vec::new(),
        }
    }

    fn class_descriptor(root: &str, classes: &[perry_hir::Class]) -> Option<Vec<u8>> {
        let table: HashMap<String, &perry_hir::Class> =
            classes.iter().map(|c| (c.name.clone(), c)).collect();
        let ids: HashMap<String, u32> = classes.iter().map(|c| (c.name.clone(), c.id)).collect();
        descriptor_for_type(
            &Type::Named(root.to_string()),
            &HashMap::new(),
            &HashMap::new(),
            &table,
            &ids,
        )
    }

    /// #8099: a class parameter carries BOTH halves — the non-zero class id
    /// that `param_type_guard.rs`'s `class_chain_reaches` branch consumes (its
    /// only caller; codegen emitted a literal 0 there until this landed), and
    /// the declared field types, which are the half that actually buys a
    /// lowering. Identity alone was measured and reverted: the clone came out
    /// structurally identical to the `$generic` sibling it routed around.
    #[test]
    fn a_class_descriptor_carries_its_class_id_and_its_declared_fields() {
        let descriptor = class_descriptor(
            "Label",
            &[class(
                7,
                "Label",
                None,
                vec![("label", Type::String), ("count", Type::Number)],
            )],
        )
        .expect("a plain class is guardable");
        // OP_OBJECT is opcode 11, then class_id: u32, then field_count: u32.
        let object = descriptor
            .windows(9)
            .find(|window| window[0] == 11)
            .expect("an object node");
        assert_eq!(
            u32::from_le_bytes(object[1..5].try_into().unwrap()),
            7,
            "the class id must reach the descriptor, or the runtime's identity \
             check stays dead: {descriptor:?}"
        );
        assert_eq!(
            u32::from_le_bytes(object[5..9].try_into().unwrap()),
            2,
            "both declared fields must be validated: {descriptor:?}"
        );
        assert!(
            descriptor.windows(5).any(|w| w == b"label"),
            "field names are validated by name against `keys_array`: {descriptor:?}"
        );
    }

    /// Inherited fields belong to the instance, so a proof that names only the
    /// leaf's own fields would license a parent field's declared type without
    /// having validated it.
    #[test]
    fn a_subclass_descriptor_validates_the_whole_inheritance_chain() {
        let descriptor = class_descriptor(
            "Derived",
            &[
                class(3, "Base", None, vec![("base", Type::String)]),
                class(4, "Derived", Some("Base"), vec![("own", Type::Number)]),
            ],
        )
        .expect("a subclass with a resolvable base is guardable");
        let object = descriptor
            .windows(9)
            .find(|window| window[0] == 11)
            .expect("an object node");
        assert_eq!(u32::from_le_bytes(object[1..5].try_into().unwrap()), 4);
        assert_eq!(
            u32::from_le_bytes(object[5..9].try_into().unwrap()),
            2,
            "the inherited field must be validated too: {descriptor:?}"
        );
    }

    /// A base HIR cannot see means the declared field set is not the whole
    /// truth about the instance, so the parameter stays generic.
    #[test]
    fn a_class_whose_base_is_unresolvable_stays_generic() {
        assert!(
            class_descriptor(
                "Orphan",
                &[class(
                    5,
                    "Orphan",
                    Some("SomeImportedThing"),
                    vec![("x", Type::Number)]
                )],
            )
            .is_none(),
            "an unresolvable parent must refuse the descriptor"
        );
    }

    /// An accessor that shares a field's name owns that property for normal JS
    /// semantics, so validating it as a data field would license a read that
    /// runs user code.
    #[test]
    fn a_class_with_an_accessor_shadowing_a_field_stays_generic() {
        let mut shadowed = class(6, "Shadowed", None, vec![("value", Type::Number)]);
        shadowed.getters.push((
            "value".to_string(),
            perry_hir::Function {
                id: 60,
                name: "get_value".to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: Type::Number,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ));
        assert!(
            class_descriptor("Shadowed", &[shadowed]).is_none(),
            "an accessor shadowing a declared field must refuse the descriptor"
        );
    }

    /// A generic class's fields are still `T`, so nothing about them is
    /// validatable until monomorphization has substituted them.
    #[test]
    fn a_generic_class_stays_generic() {
        let mut generic = class(8, "Holder", None, vec![("item", Type::Number)]);
        generic.type_params.push(perry_hir::types::TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        });
        assert!(
            class_descriptor("Holder", &[generic]).is_none(),
            "an unsubstituted generic class must refuse the descriptor"
        );
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
