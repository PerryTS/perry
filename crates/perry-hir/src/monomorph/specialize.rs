use super::*;

/// Create a specialized version of a function
pub fn specialize_function(func: &Function, type_args: &[Type], new_id: FuncId) -> Function {
    // Build substitution map from type params to concrete types
    let substitutions: HashMap<String, Type> = func
        .type_params
        .iter()
        .zip(type_args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect();

    // Generate specialized name
    let specialized_name = generate_specialized_name(&func.name, type_args);

    Function {
        id: new_id,
        name: specialized_name,
        type_params: Vec::new(), // Specialized function has no type params
        params: func
            .params
            .iter()
            .map(|p| Param {
                id: p.id,
                name: p.name.clone(),
                ty: substitute_type(&p.ty, &substitutions),
                default: p
                    .default
                    .as_ref()
                    .map(|d| substitute_expr(d, &substitutions)),
                decorators: p.decorators.clone(),
                is_rest: p.is_rest,
                arguments_object: p.arguments_object.clone(),
            })
            .collect(),
        return_type: substitute_type(&func.return_type, &substitutions),
        body: substitute_stmts(&func.body, &substitutions),
        is_async: func.is_async,
        is_generator: func.is_generator,
        is_strict: func.is_strict,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false, // Specialized versions are internal
        captures: func.captures.clone(),
        decorators: func.decorators.clone(),
    }
}

/// Create a specialized version of a class
pub fn specialize_class(class: &Class, type_args: &[Type], new_id: ClassId) -> Class {
    // Build substitution map from type params to concrete types
    let substitutions: HashMap<String, Type> = class
        .type_params
        .iter()
        .zip(type_args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect();

    // Generate specialized name
    let specialized_name = generate_specialized_name(&class.name, type_args);
    let ctor_name = format!("{}::constructor", specialized_name);

    Class {
        id: new_id,
        name: specialized_name,
        type_params: Vec::new(), // Specialized class has no type params
        extends: class.extends,  // TODO: Handle generic extends
        extends_name: class.extends_name.clone(),
        native_extends: class.native_extends.clone(),
        extends_expr: class.extends_expr.clone(),
        heritage_lexically_shadowed: class.heritage_lexically_shadowed,
        fields: class
            .fields
            .iter()
            .map(|f| ClassField {
                name: f.name.clone(),
                key_expr: f
                    .key_expr
                    .as_ref()
                    .map(|e| substitute_expr(e, &substitutions)),
                ty: substitute_type(&f.ty, &substitutions),
                init: f.init.as_ref().map(|e| substitute_expr(e, &substitutions)),
                is_private: f.is_private,
                is_readonly: f.is_readonly,
                decorators: f.decorators.clone(),
            })
            .collect(),
        constructor: class.constructor.as_ref().map(|ctor| Function {
            id: ctor.id,
            name: ctor_name.clone(),
            type_params: Vec::new(),
            params: ctor
                .params
                .iter()
                .map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, &substitutions),
                    default: p
                        .default
                        .as_ref()
                        .map(|d| substitute_expr(d, &substitutions)),
                    decorators: p.decorators.clone(),
                    is_rest: p.is_rest,
                    arguments_object: p.arguments_object.clone(),
                })
                .collect(),
            return_type: Type::Void,
            body: substitute_stmts(&ctor.body, &substitutions),
            is_async: false,
            is_generator: false,
            is_strict: ctor.is_strict,
            was_plain_async: false,
            was_unrolled: false,
            is_exported: false,
            captures: ctor.captures.clone(),
            decorators: ctor.decorators.clone(),
        }),
        methods: class
            .methods
            .iter()
            .map(|m| {
                Function {
                    id: m.id,
                    name: m.name.clone(),
                    type_params: m.type_params.clone(), // Methods can still be generic
                    params: m
                        .params
                        .iter()
                        .map(|p| Param {
                            id: p.id,
                            name: p.name.clone(),
                            ty: substitute_type(&p.ty, &substitutions),
                            default: p
                                .default
                                .as_ref()
                                .map(|d| substitute_expr(d, &substitutions)),
                            decorators: p.decorators.clone(),
                            is_rest: p.is_rest,
                            arguments_object: p.arguments_object.clone(),
                        })
                        .collect(),
                    return_type: substitute_type(&m.return_type, &substitutions),
                    body: substitute_stmts(&m.body, &substitutions),
                    is_async: m.is_async,
                    is_generator: m.is_generator,
                    is_strict: m.is_strict,
                    was_plain_async: false,
                    was_unrolled: false,
                    is_exported: false,
                    captures: m.captures.clone(),
                    decorators: m.decorators.clone(),
                }
            })
            .collect(),
        getters: class
            .getters
            .iter()
            .map(|(name, f)| {
                (
                    name.clone(),
                    Function {
                        id: f.id,
                        name: f.name.clone(),
                        type_params: Vec::new(),
                        params: Vec::new(),
                        return_type: substitute_type(&f.return_type, &substitutions),
                        body: substitute_stmts(&f.body, &substitutions),
                        is_async: false,
                        is_generator: false,
                        is_strict: f.is_strict,
                        was_plain_async: false,
                        was_unrolled: false,
                        is_exported: false,
                        captures: f.captures.clone(),
                        decorators: f.decorators.clone(),
                    },
                )
            })
            .collect(),
        setters: class
            .setters
            .iter()
            .map(|(name, f)| {
                (
                    name.clone(),
                    Function {
                        id: f.id,
                        name: f.name.clone(),
                        type_params: Vec::new(),
                        params: f
                            .params
                            .iter()
                            .map(|p| Param {
                                id: p.id,
                                name: p.name.clone(),
                                ty: substitute_type(&p.ty, &substitutions),
                                default: p
                                    .default
                                    .as_ref()
                                    .map(|d| substitute_expr(d, &substitutions)),
                                decorators: p.decorators.clone(),
                                is_rest: p.is_rest,
                                arguments_object: p.arguments_object.clone(),
                            })
                            .collect(),
                        return_type: Type::Void,
                        body: substitute_stmts(&f.body, &substitutions),
                        is_async: false,
                        is_generator: false,
                        is_strict: f.is_strict,
                        was_plain_async: false,
                        was_unrolled: false,
                        is_exported: false,
                        captures: f.captures.clone(),
                        decorators: f.decorators.clone(),
                    },
                )
            })
            .collect(),
        static_accessor_names: class.static_accessor_names.clone(),
        static_accessor_fn_ids: class.static_accessor_fn_ids.clone(),
        static_fields: class.static_fields.clone(),
        static_methods: class.static_methods.clone(),
        computed_members: class
            .computed_members
            .iter()
            .map(|member| ClassComputedMember {
                key_expr: substitute_expr(&member.key_expr, &substitutions),
                function: Function {
                    id: member.function.id,
                    name: member.function.name.clone(),
                    type_params: member.function.type_params.clone(),
                    params: member
                        .function
                        .params
                        .iter()
                        .map(|p| Param {
                            id: p.id,
                            name: p.name.clone(),
                            ty: substitute_type(&p.ty, &substitutions),
                            default: p
                                .default
                                .as_ref()
                                .map(|d| substitute_expr(d, &substitutions)),
                            decorators: p.decorators.clone(),
                            is_rest: p.is_rest,
                            arguments_object: p.arguments_object.clone(),
                        })
                        .collect(),
                    return_type: substitute_type(&member.function.return_type, &substitutions),
                    body: substitute_stmts(&member.function.body, &substitutions),
                    is_async: member.function.is_async,
                    is_generator: member.function.is_generator,
                    is_strict: member.function.is_strict,
                    was_plain_async: member.function.was_plain_async,
                    was_unrolled: member.function.was_unrolled,
                    is_exported: false,
                    captures: member.function.captures.clone(),
                    decorators: member.function.decorators.clone(),
                },
                is_static: member.is_static,
                kind: member.kind,
            })
            .collect(),
        decorators: class.decorators.clone(),
        is_exported: class.is_exported,
        aliases: class.aliases.clone(),
        is_nested: class.is_nested,
    }
}
