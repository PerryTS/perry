//! HIR-backed type facts for codegen type analysis.

use perry_hir::types::Type as HirType;
use perry_hir::{infer_expr_type, infer_refinable_expr_type, Expr, HirTypeFacts};

use crate::expr::FnCtx;

pub(crate) struct CodegenTypeFacts<'a> {
    pub(crate) local_types: &'a std::collections::HashMap<u32, HirType>,
    pub(crate) imported_func_return_types: &'a std::collections::HashMap<String, HirType>,
    pub(crate) classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    pub(crate) interfaces: &'a std::collections::HashMap<String, perry_hir::Interface>,
    pub(crate) class_stack: &'a [String],
    pub(crate) enums: &'a std::collections::HashMap<(String, String), perry_hir::EnumValue>,
}

impl<'a> CodegenTypeFacts<'a> {
    pub(crate) fn from_ctx(ctx: &'a FnCtx<'a>) -> Self {
        Self {
            local_types: &ctx.local_types,
            imported_func_return_types: ctx.imported_func_return_types,
            classes: ctx.classes,
            interfaces: ctx.interfaces,
            class_stack: &ctx.class_stack,
            enums: ctx.enums,
        }
    }
}

impl HirTypeFacts for CodegenTypeFacts<'_> {
    fn local_type(&self, id: u32) -> Option<&HirType> {
        self.local_types.get(&id)
    }

    fn global_type(&self, _id: u32) -> Option<&HirType> {
        None
    }

    fn function_return_type(&self, _id: u32) -> Option<&HirType> {
        // Intentionally conservative: codegen doesn't thread a local
        // function-return-type map through `FnCtx`, so direct `FuncRef` calls
        // infer `Any` rather than a possibly-stale declared type. Wiring the
        // module's `Function.return_type` map in is a precision follow-up, not a
        // correctness fix. Locked in by `function_return_type_is_conservative`.
        None
    }

    fn extern_function_return_type(&self, name: &str) -> Option<&HirType> {
        self.imported_func_return_types.get(name)
    }

    fn this_type(&self) -> Option<HirType> {
        self.class_stack.last().cloned().map(HirType::Named)
    }

    fn enum_member_type(&self, enum_name: &str, member_name: &str) -> Option<HirType> {
        match self
            .enums
            .get(&(enum_name.to_string(), member_name.to_string()))?
        {
            perry_hir::EnumValue::Number(_) => Some(HirType::Number),
            perry_hir::EnumValue::String(_) => Some(HirType::String),
        }
    }

    fn static_field_type(&self, class_name: &str, field_name: &str) -> Option<&HirType> {
        lookup_codegen_static_field(self.classes, class_name, field_name)
    }

    fn static_method_return_type(&self, class_name: &str, method_name: &str) -> Option<&HirType> {
        lookup_codegen_static_method_return(self.classes, class_name, method_name)
    }

    fn named_property_type(&self, type_name: &str, property: &str) -> Option<HirType> {
        lookup_codegen_named_property(self.classes, self.interfaces, type_name, property)
    }

    fn super_property_type(&self, property: &str) -> Option<HirType> {
        let current_class = self.class_stack.last()?;
        lookup_codegen_super_property(self.classes, current_class, property)
    }

    fn super_method_return_type(&self, method: &str) -> Option<HirType> {
        let current_class = self.class_stack.last()?;
        lookup_codegen_super_method_return(self.classes, current_class, method).cloned()
    }
}

#[cfg(test)]
pub(crate) fn hir_inferred_refinable_type_from_locals(
    local_types: &std::collections::HashMap<u32, HirType>,
    expr: &Expr,
) -> Option<HirType> {
    infer_refinable_expr_type(expr, local_types)
}

pub(crate) fn hir_inferred_refinable_type_from_facts(
    facts: &impl HirTypeFacts,
    expr: &Expr,
) -> Option<HirType> {
    infer_refinable_expr_type(expr, facts)
}

pub(crate) fn hir_inferred_refinable_type(ctx: &FnCtx<'_>, expr: &Expr) -> Option<HirType> {
    let facts = CodegenTypeFacts::from_ctx(ctx);
    hir_inferred_refinable_type_from_facts(&facts, expr)
}

#[cfg(test)]
pub(crate) fn hir_inferred_static_type_from_locals(
    local_types: &std::collections::HashMap<u32, HirType>,
    expr: &Expr,
) -> Option<HirType> {
    match infer_expr_type(expr, local_types) {
        HirType::Any | HirType::Unknown => None,
        ty => Some(ty),
    }
}

pub(crate) fn hir_inferred_static_type(ctx: &FnCtx<'_>, expr: &Expr) -> Option<HirType> {
    let facts = CodegenTypeFacts::from_ctx(ctx);
    match infer_expr_type(expr, &facts) {
        HirType::Any | HirType::Unknown => None,
        ty => Some(ty),
    }
}

pub(crate) fn function_type_from_decl(function: &perry_hir::Function) -> HirType {
    HirType::Function(perry_hir::types::FunctionType {
        params: function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone(), false))
            .collect(),
        return_type: Box::new(function.return_type.clone()),
        is_async: function.is_async || function.was_plain_async,
        is_generator: function.is_generator,
    })
}

fn interface_method_type(method: &perry_hir::InterfaceMethod) -> HirType {
    HirType::Function(perry_hir::types::FunctionType {
        params: method.params.clone(),
        return_type: Box::new(method.return_type.clone()),
        is_async: false,
        is_generator: false,
    })
}

fn named_type_base(ty: &HirType) -> Option<&str> {
    match ty {
        HirType::Named(name) => Some(name),
        HirType::Generic { base, .. } => Some(base),
        _ => None,
    }
}

fn codegen_parent_class_name<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class: &'a perry_hir::Class,
) -> Option<&'a str> {
    if let Some(parent) = class.extends_name.as_deref() {
        return Some(parent);
    }
    let parent_id = class.extends?;
    classes
        .values()
        .copied()
        .find(|candidate| candidate.id == parent_id)
        .map(|parent| parent.name.as_str())
}

fn lookup_codegen_static_field<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class_name: &str,
    field_name: &str,
) -> Option<&'a HirType> {
    let mut visited = std::collections::HashSet::new();
    lookup_codegen_static_field_inner(classes, class_name, field_name, &mut visited)
}

fn lookup_codegen_static_field_inner<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class_name: &str,
    field_name: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<&'a HirType> {
    let class = classes.get(class_name)?;
    if let Some(field) = class
        .static_fields
        .iter()
        .find(|field| field.name == field_name)
    {
        return Some(&field.ty);
    }
    if !visited.insert(class_name.to_string()) {
        return None;
    }
    let parent = codegen_parent_class_name(classes, class)?;
    lookup_codegen_static_field_inner(classes, parent, field_name, visited)
}

fn lookup_codegen_static_method_return<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class_name: &str,
    method_name: &str,
) -> Option<&'a HirType> {
    let mut visited = std::collections::HashSet::new();
    lookup_codegen_static_method_return_inner(classes, class_name, method_name, &mut visited)
}

fn lookup_codegen_static_method_return_inner<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class_name: &str,
    method_name: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<&'a HirType> {
    let class = classes.get(class_name)?;
    if let Some(method) = class
        .static_methods
        .iter()
        .find(|method| method.name == method_name)
    {
        return Some(&method.return_type);
    }
    if !visited.insert(class_name.to_string()) {
        return None;
    }
    let parent = codegen_parent_class_name(classes, class)?;
    lookup_codegen_static_method_return_inner(classes, parent, method_name, visited)
}

fn lookup_codegen_super_property(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    current_class_name: &str,
    property: &str,
) -> Option<HirType> {
    let current = classes.get(current_class_name)?;
    let parent = codegen_parent_class_name(classes, current)?;
    let mut visited = std::collections::HashSet::new();
    lookup_codegen_super_property_inner(classes, parent, property, &mut visited)
}

fn lookup_codegen_super_property_inner(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
    property: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<HirType> {
    if !visited.insert(class_name.to_string()) {
        return None;
    }
    let class = classes.get(class_name)?;
    if let Some((_, getter)) = class.getters.iter().find(|(name, getter)| {
        name == property && !class.static_accessor_fn_ids.contains(&getter.id)
    }) {
        return Some(getter.return_type.clone());
    }
    if let Some(method) = class.methods.iter().find(|method| method.name == property) {
        return Some(function_type_from_decl(method));
    }
    let parent = codegen_parent_class_name(classes, class)?;
    lookup_codegen_super_property_inner(classes, parent, property, visited)
}

fn lookup_codegen_super_method_return<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    current_class_name: &str,
    method: &str,
) -> Option<&'a HirType> {
    let current = classes.get(current_class_name)?;
    let parent = codegen_parent_class_name(classes, current)?;
    let mut visited = std::collections::HashSet::new();
    lookup_codegen_super_method_return_inner(classes, parent, method, &mut visited)
}

fn lookup_codegen_super_method_return_inner<'a>(
    classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    class_name: &str,
    method_name: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<&'a HirType> {
    if !visited.insert(class_name.to_string()) {
        return None;
    }
    let class = classes.get(class_name)?;
    if let Some(method) = class
        .methods
        .iter()
        .find(|method| method.name == method_name)
    {
        return Some(&method.return_type);
    }
    let parent = codegen_parent_class_name(classes, class)?;
    lookup_codegen_super_method_return_inner(classes, parent, method_name, visited)
}

fn lookup_codegen_named_property(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    interfaces: &std::collections::HashMap<String, perry_hir::Interface>,
    type_name: &str,
    property: &str,
) -> Option<HirType> {
    let mut visited = std::collections::HashSet::new();
    lookup_codegen_named_property_inner(classes, interfaces, type_name, property, &mut visited)
}

fn lookup_codegen_named_property_inner(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    interfaces: &std::collections::HashMap<String, perry_hir::Interface>,
    type_name: &str,
    property: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<HirType> {
    if !visited.insert(type_name.to_string()) {
        return None;
    }

    if let Some(class) = classes.get(type_name) {
        if let Some(field) = class.fields.iter().find(|field| field.name == property) {
            return Some(field.ty.clone());
        }
        if let Some((_, getter)) = class.getters.iter().find(|(name, getter)| {
            name == property && !class.static_accessor_fn_ids.contains(&getter.id)
        }) {
            return Some(getter.return_type.clone());
        }
        if let Some(method) = class.methods.iter().find(|method| method.name == property) {
            return Some(function_type_from_decl(method));
        }
        if let Some(parent) = codegen_parent_class_name(classes, class) {
            if let Some(ty) =
                lookup_codegen_named_property_inner(classes, interfaces, parent, property, visited)
            {
                return Some(ty);
            }
        }
    }

    if let Some(interface) = interfaces.get(type_name) {
        if let Some(prop) = interface
            .properties
            .iter()
            .find(|prop| prop.name == property)
        {
            return Some(prop.ty.clone());
        }
        if let Some(method) = interface
            .methods
            .iter()
            .find(|method| method.name == property)
        {
            return Some(interface_method_type(method));
        }
        for parent in interface.extends.iter().filter_map(named_type_base) {
            if let Some(ty) =
                lookup_codegen_named_property_inner(classes, interfaces, parent, property, visited)
            {
                return Some(ty);
            }
        }
    }

    None
}
