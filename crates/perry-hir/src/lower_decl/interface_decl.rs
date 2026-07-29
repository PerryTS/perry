use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::LoweringContext;
use crate::lower_types::*;

pub fn lower_interface_decl(
    ctx: &mut LoweringContext,
    iface_decl: &ast::TsInterfaceDecl,
    is_exported: bool,
) -> Result<Interface> {
    let name = iface_decl.id.sym.to_string();
    let iface_id = ctx.fresh_interface();

    // Extract type parameters
    let type_params = iface_decl
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references in body
    ctx.enter_type_param_scope(&type_params);

    // Extract extended interfaces
    let extends: Vec<Type> = iface_decl
        .extends
        .iter()
        .map(|ext| {
            let base_name = match &*ext.expr {
                ast::Expr::Ident(id) => id.sym.to_string(),
                _ => "unknown".to_string(),
            };
            // Handle type arguments if present
            if let Some(ref type_args) = ext.type_args {
                let args: Vec<Type> = type_args
                    .params
                    .iter()
                    .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                    .collect();
                if args.is_empty() {
                    Type::Named(base_name)
                } else {
                    Type::Generic {
                        base: base_name,
                        type_args: args,
                    }
                }
            } else {
                Type::Named(base_name)
            }
        })
        .collect();

    // Extract properties and methods from interface body
    let mut properties = Vec::new();
    let mut methods = Vec::new();

    for member in &iface_decl.body.body {
        match member {
            ast::TsTypeElement::TsPropertySignature(prop) => {
                let prop_name = match &*prop.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                let prop_type = prop
                    .type_ann
                    .as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Any);
                properties.push(InterfaceProperty {
                    name: prop_name,
                    ty: prop_type,
                    optional: prop.optional,
                    readonly: prop.readonly,
                });
            }
            ast::TsTypeElement::TsMethodSignature(method) => {
                let method_name = match &*method.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };

                // Method's own type parameters
                let method_type_params = method
                    .type_params
                    .as_ref()
                    .map(|tp| extract_type_params(tp))
                    .unwrap_or_default();

                // Enter method's type param scope
                ctx.enter_type_param_scope(&method_type_params);

                // Extract parameters
                let params: Vec<(String, Type, bool)> = method
                    .params
                    .iter()
                    .map(|p| {
                        let (name, ty) = get_fn_param_name_and_type_with_ctx(p, Some(ctx));
                        let optional = matches!(p, ast::TsFnParam::Ident(id) if id.optional);
                        (name, ty, optional)
                    })
                    .collect();

                // Extract return type
                let return_type = method
                    .type_ann
                    .as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Void);

                ctx.exit_type_param_scope();

                methods.push(InterfaceMethod {
                    name: method_name,
                    type_params: method_type_params,
                    params,
                    return_type,
                });
            }
            _ => {} // Skip other member types for now
        }
    }

    ctx.exit_type_param_scope();

    // Register interface in context
    ctx.interfaces.push((name.clone(), iface_id));

    // Issue #179 typed-parse: record field names in source order so
    // `JSON.parse<Name[]>` codegen can emit a shape hint that matches
    // how `JSON.stringify` lays them out on the wire.
    let source_keys: Vec<String> = properties.iter().map(|p| p.name.clone()).collect();
    if !source_keys.is_empty() {
        ctx.interface_source_keys
            .insert(name.clone(), source_keys.clone());
    }
    // Also materialize an ObjectType so `resolve_typed_parse_ty` can
    // expand `Named("Item")` → `Object{fields}` for codegen.
    let mut obj_props: std::collections::HashMap<String, crate::types::PropertyInfo> =
        std::collections::HashMap::new();
    for p in &properties {
        obj_props.insert(
            p.name.clone(),
            crate::types::PropertyInfo {
                ty: p.ty.clone(),
                optional: p.optional,
                readonly: p.readonly,
            },
        );
    }
    if !obj_props.is_empty() {
        ctx.interface_object_types.insert(
            name.clone(),
            crate::types::ObjectType {
                name: Some(name.clone()),
                properties: obj_props,
                property_order: Some(source_keys.clone()),
                index_signature: None,
            },
        );
    }

    Ok(Interface {
        id: iface_id,
        name,
        type_params,
        extends,
        properties,
        methods,
        is_exported,
    })
}
