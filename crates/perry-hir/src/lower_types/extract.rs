//! TypeScript annotation extraction — `extract_ts_type` and friends, plus
//! decorator lowering. Split out of `lower_types.rs` (#6233 follow-up) to
//! keep the parent under the 2000-line lint cap; pure move, no logic change.
//! Named re-exports in `lower_types.rs` keep every existing call path
//! (`crate::lower_types::extract_ts_type`, ...) compiling unchanged.

use perry_types::{Type, TypeParam};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::{get_pat_name, lower_lit};

/// Extract type parameters from SWC's TsTypeParamDecl
pub(crate) fn extract_type_params(decl: &ast::TsTypeParamDecl) -> Vec<TypeParam> {
    decl.params
        .iter()
        .map(|p| {
            let name = p.name.sym.to_string();
            let constraint = p.constraint.as_ref().map(|c| Box::new(extract_ts_type(c)));
            let default = p.default.as_ref().map(|d| Box::new(extract_ts_type(d)));
            TypeParam {
                name,
                constraint,
                default,
            }
        })
        .collect()
}

/// Extract a Type from an SWC TypeScript type annotation
/// This version doesn't have access to type parameter context
pub(crate) fn extract_ts_type(ts_type: &ast::TsType) -> Type {
    extract_ts_type_with_ctx(ts_type, None)
}

/// Extract a Type from an SWC TypeScript type annotation with type parameter context
pub(crate) fn extract_ts_type_with_ctx(
    ts_type: &ast::TsType,
    ctx: Option<&LoweringContext>,
) -> Type {
    use ast::TsKeywordTypeKind::*;
    use ast::TsType::*;

    match ts_type {
        // Keyword types (primitives)
        TsKeywordType(kw) => match kw.kind {
            TsNumberKeyword => Type::Number,
            TsStringKeyword => Type::String,
            TsBooleanKeyword => Type::Boolean,
            TsBigIntKeyword => Type::BigInt,
            TsVoidKeyword => Type::Void,
            TsNullKeyword => Type::Null,
            TsUndefinedKeyword => Type::Void,
            TsAnyKeyword => Type::Any,
            TsUnknownKeyword => Type::Unknown,
            TsNeverKeyword => Type::Never,
            TsSymbolKeyword => Type::Symbol,
            TsObjectKeyword => Type::Any, // Generic object
            TsIntrinsicKeyword => Type::Any,
        },

        // Array type: T[]
        TsArrayType(arr) => {
            let elem_type = extract_ts_type_with_ctx(&arr.elem_type, ctx);
            Type::Array(Box::new(elem_type))
        }

        // Tuple type: [T, U, V]
        TsTupleType(tuple) => {
            let elem_types: Vec<Type> = tuple
                .elem_types
                .iter()
                .map(|elem| extract_ts_type_with_ctx(&elem.ty, ctx))
                .collect();
            Type::Tuple(elem_types)
        }

        // Union type: A | B | C
        TsUnionOrIntersectionType(union_or_inter) => {
            match union_or_inter {
                ast::TsUnionOrIntersectionType::TsUnionType(union) => {
                    let types: Vec<Type> = union
                        .types
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, ctx))
                        .collect();
                    Type::Union(types)
                }
                ast::TsUnionOrIntersectionType::TsIntersectionType(_) => {
                    // Intersection types are complex - treat as Any for now
                    Type::Any
                }
            }
        }

        // Type reference: Array<T>, MyClass, T (type param), etc.
        TsTypeRef(type_ref) => {
            let name = match &type_ref.type_name {
                ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                ast::TsEntityName::TsQualifiedName(qname) => {
                    // Qualified names like Foo.Bar
                    format!("{}.{}", get_ts_entity_name(&qname.left), qname.right.sym)
                }
            };

            // First check if this is a type parameter reference (like T, K, V).
            //
            // When the parameter has a runtime-meaningful upper-bound
            // constraint (`<T extends string>`, `<T extends number>`,
            // `<T extends string[]>` …) substitute the constraint type
            // here, so the rest of the lowering + codegen sees the
            // narrowed runtime type directly. Without this, perry's
            // codegen `is_string_expr`/`is_array_expr`/`is_numeric_expr`
            // fast paths don't fire on `<T extends string>(self: T)
            // => self[0]` and the IndexGet falls through to the
            // polymorphic-object runtime helper, which reads a
            // `StringHeader*` as `ArrayHeader*` and returns header
            // bytes as a subnormal f64 (#321: effect `Str.capitalize`
            // surfaced as `1.5E-323oo`). Arrow functions and
            // function-typed-local indirections in particular bypass
            // generic-call monomorphization entirely, so the
            // un-substituted body would be the one codegen emits.
            //
            // Constraints that don't usefully narrow the runtime
            // representation (named class, literal type, intersection,
            // `unknown`/`any`) fall through to `TypeVar(name)` as
            // before — preserving the existing native-instance tagging
            // / class-id propagation paths.
            if let Some(context) = ctx {
                if context.is_type_param(&name) {
                    if let Some(resolved) = context.resolve_type_param_constraint(&name) {
                        return resolved;
                    }
                    return Type::TypeVar(name);
                }
            }

            // Check for built-in generic types or generic instantiations
            if let Some(type_params) = &type_ref.type_params {
                match name.as_str() {
                    "Array" if !type_params.params.is_empty() => {
                        let elem_type = extract_ts_type_with_ctx(&type_params.params[0], ctx);
                        return Type::Array(Box::new(elem_type));
                    }
                    "Promise" if !type_params.params.is_empty() => {
                        let result_type = extract_ts_type_with_ctx(&type_params.params[0], ctx);
                        return Type::Promise(Box::new(result_type));
                    }
                    _ => {
                        // Generic type instantiation (e.g., Box<number>, Map<string, number>)
                        let type_args: Vec<Type> = type_params
                            .params
                            .iter()
                            .map(|t| extract_ts_type_with_ctx(t, ctx))
                            .collect();
                        return Type::Generic {
                            base: name,
                            type_args,
                        };
                    }
                }
            }

            if matches!(
                name.as_str(),
                "PerryU32"
                    | "PerryU64"
                    | "PerryUSize"
                    | "PerryF32"
                    | "PerryF64"
                    | "PerryI32"
                    | "PerryI64"
                    | "PerryBufferLen"
                    | "PerryHandleId"
            ) {
                return Type::Named(name);
            }

            // Check if this is a type alias — resolve to the underlying type
            // so the codegen sees Union/String/Number instead of Named("BlockTag").
            // Without this, `type BlockTag = 'latest' | number | string` stays as
            // Named("BlockTag") which the codegen treats as I64 (object pointer),
            // causing ABI mismatch when the actual value is a NaN-boxed union.
            if let Some(context) = ctx {
                if let Some(resolved) = context.resolve_type_alias(&name) {
                    return resolved;
                }
            }

            Type::Named(name)
        }

        // Function type: (a: T, b: U) => R
        TsFnOrConstructorType(fn_type) => {
            match fn_type {
                ast::TsFnOrConstructorType::TsFnType(fn_ty) => {
                    // Extract parameter types
                    let params: Vec<(String, Type, bool)> = fn_ty
                        .params
                        .iter()
                        .map(|p| {
                            let (name, ty) = get_fn_param_name_and_type_with_ctx(p, ctx);
                            (name, ty, false) // TODO: detect optional params
                        })
                        .collect();

                    let return_type = extract_ts_type_with_ctx(&fn_ty.type_ann.type_ann, ctx);

                    Type::Function(perry_types::FunctionType {
                        params,
                        return_type: Box::new(return_type),
                        is_async: false,
                        is_generator: false,
                    })
                }
                ast::TsFnOrConstructorType::TsConstructorType(_) => {
                    // Constructor types are complex - treat as Any for now
                    Type::Any
                }
            }
        }

        // Literal types: "foo", 42, true
        TsLitType(lit) => match &lit.lit {
            ast::TsLit::Number(_) => Type::Number,
            ast::TsLit::Str(s) => Type::StringLiteral(s.value.as_str().unwrap_or("").to_string()),
            ast::TsLit::Bool(_) => Type::Boolean,
            ast::TsLit::BigInt(_) => Type::BigInt,
            ast::TsLit::Tpl(_) => Type::String,
        },

        // Parenthesized type: (T)
        TsParenthesizedType(paren) => extract_ts_type_with_ctx(&paren.type_ann, ctx),

        // Optional type: T?
        TsOptionalType(opt) => extract_ts_type_with_ctx(&opt.type_ann, ctx),

        // Rest type: ...T
        TsRestType(rest) => extract_ts_type_with_ctx(&rest.type_ann, ctx),

        // Type query: typeof x
        TsTypeQuery(_) => Type::Any,

        // Conditional type: T extends U ? X : Y
        TsConditionalType(_) => Type::Any,

        // Mapped type: { [K in T]: U }
        TsMappedType(_) => Type::Any,

        // Index access: T[K]
        TsIndexedAccessType(_) => Type::Any,

        // Infer type: infer T
        TsInferType(_) => Type::Any,

        // this type
        TsThisType(_) => Type::Any,

        // Type predicate: x is T
        TsTypePredicate(_) => Type::Boolean,

        // Import type: import("module").Type
        TsImportType(_) => Type::Any,

        // Type operator: keyof T, readonly T, unique symbol.
        // For `readonly T` we just return the inner type (the readonly
        // modifier is purely a type-system concept; runtime treatment is
        // identical to T). keyof and unique symbol stay as Any.
        TsTypeOperator(op) => {
            use swc_ecma_ast::TsTypeOperatorOp;
            match op.op {
                TsTypeOperatorOp::ReadOnly => extract_ts_type_with_ctx(&op.type_ann, ctx),
                TsTypeOperatorOp::KeyOf => Type::String,
                _ => Type::Any,
            }
        }

        // Type literal: { a: T, b: U }
        TsTypeLit(lit) => {
            let mut properties = std::collections::HashMap::new();
            let mut property_order = Vec::new();
            for member in &lit.members {
                match member {
                    ast::TsTypeElement::TsPropertySignature(prop) => {
                        if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                            let field_name = ident.sym.to_string();
                            let field_type = if let Some(ann) = &prop.type_ann {
                                extract_ts_type_with_ctx(&ann.type_ann, ctx)
                            } else {
                                Type::Any
                            };
                            if !properties.contains_key(&field_name) {
                                property_order.push(field_name.clone());
                            }
                            properties.insert(
                                field_name,
                                perry_types::PropertyInfo {
                                    ty: field_type,
                                    optional: prop.optional,
                                    readonly: prop.readonly,
                                },
                            );
                        }
                    }
                    ast::TsTypeElement::TsMethodSignature(method) => {
                        if let ast::Expr::Ident(ident) = method.key.as_ref() {
                            let method_name = ident.sym.to_string();
                            let return_type = method
                                .type_ann
                                .as_ref()
                                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                                .unwrap_or(Type::Any);
                            let params: Vec<(String, Type, bool)> = method
                                .params
                                .iter()
                                .map(|p| {
                                    let (name, ty) = get_fn_param_name_and_type_with_ctx(p, ctx);
                                    (name, ty, false)
                                })
                                .collect();
                            properties.insert(
                                method_name,
                                perry_types::PropertyInfo {
                                    ty: Type::Function(perry_types::FunctionType {
                                        params,
                                        return_type: Box::new(return_type),
                                        is_async: false,
                                        is_generator: false,
                                    }),
                                    optional: method.optional,
                                    readonly: false,
                                },
                            );
                        }
                    }
                    ast::TsTypeElement::TsIndexSignature(idx_sig) => {
                        // index signature: { [key: string]: T }
                        if let Some(ann) = &idx_sig.type_ann {
                            let val_type = extract_ts_type_with_ctx(&ann.type_ann, ctx);
                            return Type::Object(perry_types::ObjectType {
                                name: None,
                                properties,
                                property_order: Some(property_order),
                                index_signature: Some(Box::new(val_type)),
                            });
                        }
                    }
                    _ => {}
                }
            }
            if properties.is_empty() {
                Type::Any
            } else {
                Type::Object(perry_types::ObjectType {
                    name: None,
                    properties,
                    property_order: Some(property_order),
                    index_signature: None,
                })
            }
        }
    }
}

/// Helper to get name from TsEntityName
pub(crate) fn get_ts_entity_name(entity: &ast::TsEntityName) -> String {
    match entity {
        ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
        ast::TsEntityName::TsQualifiedName(qname) => {
            format!("{}.{}", get_ts_entity_name(&qname.left), qname.right.sym)
        }
    }
}

/// Helper to get parameter name and type from TsFnParam with context
pub(crate) fn get_fn_param_name_and_type_with_ctx(
    param: &ast::TsFnParam,
    ctx: Option<&LoweringContext>,
) -> (String, Type) {
    match param {
        ast::TsFnParam::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            (name, ty)
        }
        ast::TsFnParam::Array(arr) => {
            let ty = arr
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_array".to_string(), ty)
        }
        ast::TsFnParam::Rest(rest) => {
            let ty = rest
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_rest".to_string(), ty)
        }
        ast::TsFnParam::Object(obj) => {
            let ty = obj
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_obj".to_string(), ty)
        }
    }
}

/// Extract class name from a member expression (e.g., "ethers.JsonRpcProvider" -> "JsonRpcProvider")
/// This is used for extends clauses that reference external module classes
pub(crate) fn extract_member_class_name(member: &ast::MemberExpr) -> String {
    match &member.prop {
        ast::MemberProp::Ident(ident) => ident.sym.to_string(),
        ast::MemberProp::Computed(computed) => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = computed.expr.as_ref() {
                s.value.as_str().unwrap_or("UnknownClass").to_string()
            } else {
                "UnknownClass".to_string()
            }
        }
        ast::MemberProp::PrivateName(priv_name) => priv_name.name.to_string(),
    }
}

/// Extract type from a pattern (handles BindingIdent with type annotation)
/// Used for both parameter patterns and variable declaration bindings
pub(crate) fn extract_pattern_type(pat: &ast::Pat) -> Type {
    extract_pattern_type_with_ctx(pat, None)
}

/// Extract type from a pattern with type parameter context
pub(crate) fn extract_pattern_type_with_ctx(pat: &ast::Pat, ctx: Option<&LoweringContext>) -> Type {
    match pat {
        ast::Pat::Ident(ident) => ident
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Array(arr) => arr
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Rest(rest) => rest
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Object(obj) => obj
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Assign(assign) => {
            // For default parameters, get type from the left side
            extract_pattern_type_with_ctx(&assign.left, ctx)
        }
        ast::Pat::Invalid(_) | ast::Pat::Expr(_) => Type::Any,
    }
}

/// Alias for parameter type extraction with context
pub(crate) fn extract_param_type_with_ctx(pat: &ast::Pat, ctx: Option<&LoweringContext>) -> Type {
    extract_pattern_type_with_ctx(pat, ctx)
}

/// Extract type from a variable declaration binding
pub(crate) fn extract_binding_type(binding: &ast::Pat) -> Type {
    extract_pattern_type(binding)
}

/// Lower decorators from SWC AST to HIR Decorators
pub(crate) fn lower_decorators(
    ctx: &mut LoweringContext,
    decorators: &[ast::Decorator],
) -> Vec<Decorator> {
    decorators
        .iter()
        .filter_map(|dec| {
            // The decorator expression can be:
            // - Identifier: @log
            // - Call expression: @log("prefix")
            match dec.expr.as_ref() {
                ast::Expr::Ident(ident) => Some(Decorator {
                    name: ident.sym.to_string(),
                    args: Vec::new(),
                    is_factory: false,
                    is_reflect_metadata: false,
                }),
                ast::Expr::Call(call) => {
                    // Get the callee name
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::Expr::Ident(obj) = member.obj.as_ref() {
                                if obj.sym.as_ref() == "Reflect" {
                                    if let ast::MemberProp::Ident(method) = &member.prop {
                                        if method.sym.as_ref() == "metadata" {
                                            let args: Vec<Expr> = call
                                                .args
                                                .iter()
                                                .filter_map(|arg| {
                                                    if arg.spread.is_some() {
                                                        None
                                                    } else {
                                                        lower_decorator_arg(ctx, arg.expr.as_ref())
                                                    }
                                                })
                                                .collect();
                                            return Some(Decorator {
                                                name: "Reflect.metadata".to_string(),
                                                args,
                                                is_factory: true,
                                                is_reflect_metadata: true,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                            let args: Vec<Expr> = call
                                .args
                                .iter()
                                .filter_map(|arg| {
                                    if arg.spread.is_some() {
                                        None
                                    } else {
                                        lower_decorator_arg(ctx, arg.expr.as_ref())
                                    }
                                })
                                .collect();
                            return Some(Decorator {
                                name: ident.sym.to_string(),
                                args,
                                is_factory: true,
                                is_reflect_metadata: false,
                            });
                        }
                    }
                    None
                }
                _ => None,
            }
        })
        .collect()
}

fn lower_decorator_arg(ctx: &mut LoweringContext, expr: &ast::Expr) -> Option<Expr> {
    match expr {
        ast::Expr::Lit(lit) => lower_lit(lit).ok(),
        ast::Expr::Ident(ident) => match lower_expr(ctx, expr).ok() {
            Some(Expr::GlobalGet(0)) => Some(Expr::ClassRef(ident.sym.to_string())),
            // Bare built-in name `Date`/`Array`/`Object`/... now lowers
            // to `PropertyGet { GlobalGet(0), name }` (so the value-side
            // identity comparison `inst.constructor === Date` matches).
            // For decorator-arg use it's still a class ref.
            Some(Expr::PropertyGet {
                object: ref obj,
                property: _,
                ..
            }) if matches!(obj.as_ref(), Expr::GlobalGet(0)) => {
                Some(Expr::ClassRef(ident.sym.to_string()))
            }
            other => other,
        },
        ast::Expr::Array(arr) => {
            let items = arr
                .elems
                .iter()
                .map(|elem| {
                    elem.as_ref()
                        .and_then(|elem| {
                            if elem.spread.is_some() {
                                None
                            } else {
                                lower_decorator_arg(ctx, elem.expr.as_ref())
                            }
                        })
                        .unwrap_or(Expr::Undefined)
                })
                .collect();
            Some(Expr::Array(items))
        }
        ast::Expr::Object(obj) => {
            let mut fields = Vec::new();
            for prop in &obj.props {
                let ast::PropOrSpread::Prop(prop) = prop else {
                    return None;
                };
                match prop.as_ref() {
                    ast::Prop::KeyValue(kv) => {
                        let key = decorator_prop_name(&kv.key)?;
                        let value = lower_decorator_arg(ctx, kv.value.as_ref())?;
                        fields.push((key, value));
                    }
                    ast::Prop::Shorthand(ident) => {
                        let name = ident.sym.to_string();
                        let value = lower_decorator_arg(ctx, &ast::Expr::Ident(ident.clone()))?;
                        fields.push((name, value));
                    }
                    _ => return None,
                }
            }
            Some(Expr::Object(fields))
        }
        _ => lower_expr(ctx, expr).ok(),
    }
}

fn decorator_prop_name(name: &ast::PropName) -> Option<String> {
    match name {
        ast::PropName::Ident(ident) => Some(ident.sym.to_string()),
        ast::PropName::Str(s) => Some(s.value.as_str().unwrap_or("").to_string()),
        ast::PropName::Num(n) => Some(n.value.to_string()),
        _ => None,
    }
}
