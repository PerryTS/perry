use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, CallArg, Expr, Stmt};

/// Collect immutable `new` locals whose method calls can skip the per-call
/// own-method override probe.
///
/// This is deliberately narrower than scalar-replacement escape analysis:
/// the object stays heap-allocated, but the local must still not escape through
/// aliases, arguments, returns, closures, or spread calls. Plain field reads and
/// writes are allowed. Method calls are allowed only when the class
/// constructor/field initializers and the called method bodies do not
/// materialize `this` as a value or install own properties over the called
/// method names.
pub(crate) fn collect_direct_method_new_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
) -> HashMap<u32, HashSet<String>> {
    let mut candidates = HashMap::new();
    find_const_new_candidates(stmts, boxed_vars, module_globals, classes, &mut candidates);
    if candidates.is_empty() {
        return HashMap::new();
    }

    let mut state = DirectMethodUseState {
        candidates: &candidates,
        classes,
        called_methods: HashMap::new(),
        read_properties: HashMap::new(),
        written_properties: HashMap::new(),
        unsafe_fields: HashMap::new(),
        unsafe_locals: HashSet::new(),
    };
    scan_stmts(stmts, &mut state);

    let mut out = HashMap::new();
    for (local_id, methods) in state.called_methods {
        if methods.is_empty() || state.unsafe_locals.contains(&local_id) {
            continue;
        }
        if state
            .written_properties
            .get(&local_id)
            .is_some_and(|writes| !writes.is_disjoint(&methods))
        {
            continue;
        }
        let Some(class_name) = candidates.get(&local_id) else {
            continue;
        };
        if class_allows_direct_methods(class_name, &methods, classes) {
            out.insert(local_id, methods);
        }
    }
    out
}

pub(crate) fn collect_direct_field_new_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
) -> HashMap<u32, HashSet<String>> {
    let mut candidates = HashMap::new();
    find_const_new_candidates(stmts, boxed_vars, module_globals, classes, &mut candidates);
    if candidates.is_empty() {
        return HashMap::new();
    }

    let mut state = DirectMethodUseState {
        candidates: &candidates,
        classes,
        called_methods: HashMap::new(),
        read_properties: HashMap::new(),
        written_properties: HashMap::new(),
        unsafe_fields: HashMap::new(),
        unsafe_locals: HashSet::new(),
    };
    scan_stmts(stmts, &mut state);

    let mut out = HashMap::new();
    for (local_id, class_name) in &candidates {
        if state.unsafe_locals.contains(local_id) {
            continue;
        }
        if !class_allows_direct_fields(class_name, classes) {
            continue;
        }
        let mut fields = state
            .read_properties
            .get(local_id)
            .cloned()
            .unwrap_or_default();
        if let Some(written) = state.written_properties.get(local_id) {
            fields.extend(written.iter().cloned());
        }
        fields.retain(|field| {
            !state
                .unsafe_fields
                .get(local_id)
                .is_some_and(|unsafe_fields| unsafe_fields.contains(field))
                && class_field_allows_direct_access(class_name, field, classes)
        });
        if !fields.is_empty() {
            out.insert(*local_id, fields);
        }
    }
    out
}

struct DirectMethodUseState<'a> {
    candidates: &'a HashMap<u32, String>,
    classes: &'a HashMap<String, &'a perry_hir::Class>,
    called_methods: HashMap<u32, HashSet<String>>,
    read_properties: HashMap<u32, HashSet<String>>,
    written_properties: HashMap<u32, HashSet<String>>,
    unsafe_fields: HashMap<u32, HashSet<String>>,
    unsafe_locals: HashSet<u32>,
}

fn find_const_new_candidates(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
    out: &mut HashMap<u32, String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::New { class_name, .. }),
                ..
            } if !boxed_vars.contains(id)
                && !module_globals.contains_key(id)
                && classes.contains_key(class_name) =>
            {
                out.insert(*id, class_name.clone());
            }
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    find_const_new_candidates_in_expr(
                        init,
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => {
                find_const_new_candidates_in_expr(e, boxed_vars, module_globals, classes, out)
            }
            Stmt::Return(Some(e)) => {
                find_const_new_candidates_in_expr(e, boxed_vars, module_globals, classes, out)
            }
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                find_const_new_candidates_in_expr(
                    condition,
                    boxed_vars,
                    module_globals,
                    classes,
                    out,
                );
                find_const_new_candidates(then_branch, boxed_vars, module_globals, classes, out);
                if let Some(else_branch) = else_branch {
                    find_const_new_candidates(
                        else_branch,
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                find_const_new_candidates_in_expr(
                    condition,
                    boxed_vars,
                    module_globals,
                    classes,
                    out,
                );
                find_const_new_candidates(body, boxed_vars, module_globals, classes, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    find_const_new_candidates(
                        std::slice::from_ref(init.as_ref()),
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
                if let Some(condition) = condition {
                    find_const_new_candidates_in_expr(
                        condition,
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
                if let Some(update) = update {
                    find_const_new_candidates_in_expr(
                        update,
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
                find_const_new_candidates(body, boxed_vars, module_globals, classes, out);
            }
            Stmt::Labeled { body, .. } => find_const_new_candidates(
                std::slice::from_ref(body.as_ref()),
                boxed_vars,
                module_globals,
                classes,
                out,
            ),
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) | Stmt::PreallocateBoxes(_) => {}
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_const_new_candidates(body, boxed_vars, module_globals, classes, out);
                if let Some(catch) = catch {
                    find_const_new_candidates(
                        &catch.body,
                        boxed_vars,
                        module_globals,
                        classes,
                        out,
                    );
                }
                if let Some(finally) = finally {
                    find_const_new_candidates(finally, boxed_vars, module_globals, classes, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                find_const_new_candidates_in_expr(
                    discriminant,
                    boxed_vars,
                    module_globals,
                    classes,
                    out,
                );
                for case in cases {
                    if let Some(test) = &case.test {
                        find_const_new_candidates_in_expr(
                            test,
                            boxed_vars,
                            module_globals,
                            classes,
                            out,
                        );
                    }
                    find_const_new_candidates(&case.body, boxed_vars, module_globals, classes, out);
                }
            }
        }
    }
}

fn find_const_new_candidates_in_expr(
    expr: &Expr,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
    out: &mut HashMap<u32, String>,
) {
    if let Expr::Closure { body, .. } = expr {
        find_const_new_candidates(body, boxed_vars, module_globals, classes, out);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        find_const_new_candidates_in_expr(child, boxed_vars, module_globals, classes, out);
    });
}

fn scan_stmts(stmts: &[Stmt], state: &mut DirectMethodUseState<'_>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    scan_expr(init, state);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => scan_expr(e, state),
            Stmt::Return(Some(e)) => scan_expr(e, state),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                scan_expr(condition, state);
                scan_stmts(then_branch, state);
                if let Some(else_branch) = else_branch {
                    scan_stmts(else_branch, state);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                scan_expr(condition, state);
                scan_stmts(body, state);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    scan_stmts(std::slice::from_ref(init.as_ref()), state);
                }
                if let Some(condition) = condition {
                    scan_expr(condition, state);
                }
                if let Some(update) = update {
                    scan_expr(update, state);
                }
                scan_stmts(body, state);
            }
            Stmt::Labeled { body, .. } => scan_stmts(std::slice::from_ref(body.as_ref()), state),
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) | Stmt::PreallocateBoxes(_) => {}
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_stmts(body, state);
                if let Some(catch) = catch {
                    scan_stmts(&catch.body, state);
                }
                if let Some(finally) = finally {
                    scan_stmts(finally, state);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                scan_expr(discriminant, state);
                for case in cases {
                    if let Some(test) = &case.test {
                        scan_expr(test, state);
                    }
                    scan_stmts(&case.body, state);
                }
            }
        }
    }
}

fn scan_expr(expr: &Expr, state: &mut DirectMethodUseState<'_>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if let Some((local_id, method)) = candidate_method_receiver(callee, state.candidates) {
                state
                    .called_methods
                    .entry(local_id)
                    .or_default()
                    .insert(method.to_string());
                for arg in args {
                    scan_expr(arg, state);
                }
                return;
            }
            scan_expr(callee, state);
            for arg in args {
                scan_expr(arg, state);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            if let Some((local_id, _)) = candidate_method_receiver(callee, state.candidates) {
                state.unsafe_locals.insert(local_id);
            }
            scan_expr(callee, state);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => scan_expr(e, state),
                }
            }
        }
        Expr::Delete(operand) => {
            mark_candidate_refs_unsafe(operand, state);
            scan_expr(operand, state);
        }
        Expr::ObjectDefineProperty(obj, key, desc) => {
            mark_candidate_refs_unsafe(obj, state);
            scan_expr(obj, state);
            scan_expr(key, state);
            scan_expr(desc, state);
        }
        Expr::ObjectDefineProperties(target, descs) | Expr::ObjectSetPrototypeOf(target, descs) => {
            mark_candidate_refs_unsafe(target, state);
            scan_expr(target, state);
            scan_expr(descs, state);
        }
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => {
            mark_candidate_refs_unsafe(target, state);
            mark_candidate_refs_unsafe(receiver, state);
            scan_expr(target, state);
            scan_expr(key, state);
            scan_expr(value, state);
            scan_expr(receiver, state);
        }
        Expr::ReflectDelete { target, key } => {
            mark_candidate_refs_unsafe(target, state);
            scan_expr(target, state);
            scan_expr(key, state);
        }
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => {
            mark_candidate_refs_unsafe(target, state);
            scan_expr(target, state);
            scan_expr(key, state);
            scan_expr(descriptor, state);
        }
        Expr::PropertyGet { object, property } => {
            if let Expr::LocalGet(local_id) = object.as_ref() {
                if let Some(class_name) = state.candidates.get(local_id) {
                    if super::is_class_getter(state.classes, class_name, property) {
                        state.unsafe_locals.insert(*local_id);
                    }
                    state
                        .read_properties
                        .entry(*local_id)
                        .or_default()
                        .insert(property.clone());
                    return;
                }
            }
            scan_expr(object, state);
        }
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            if let Expr::LocalGet(local_id) = object.as_ref() {
                if let Some(class_name) = state.candidates.get(local_id) {
                    if super::is_class_setter(state.classes, class_name, property) {
                        state.unsafe_locals.insert(*local_id);
                    }
                    state
                        .written_properties
                        .entry(*local_id)
                        .or_default()
                        .insert(property.clone());
                    if class_field_is_raw_f64(state.classes, class_name, property)
                        && !expr_is_numeric_for_direct_field(
                            value,
                            property,
                            state.candidates,
                            state.classes,
                        )
                    {
                        state
                            .unsafe_fields
                            .entry(*local_id)
                            .or_default()
                            .insert(property.clone());
                    }
                    scan_expr(value, state);
                    return;
                }
            }
            scan_expr(object, state);
            scan_expr(value, state);
        }
        Expr::PropertyUpdate {
            object, property, ..
        } => {
            if let Expr::LocalGet(local_id) = object.as_ref() {
                if let Some(class_name) = state.candidates.get(local_id) {
                    if super::is_class_getter(state.classes, class_name, property)
                        || super::is_class_setter(state.classes, class_name, property)
                    {
                        state.unsafe_locals.insert(*local_id);
                    }
                    state
                        .written_properties
                        .entry(*local_id)
                        .or_default()
                        .insert(property.clone());
                    return;
                }
            }
            scan_expr(object, state);
        }
        Expr::LocalGet(local_id) => {
            if state.candidates.contains_key(local_id) {
                state.unsafe_locals.insert(*local_id);
            }
        }
        Expr::LocalSet(local_id, value) => {
            if state.candidates.contains_key(local_id) {
                state.unsafe_locals.insert(*local_id);
            }
            scan_expr(value, state);
        }
        Expr::Update { id, .. } => {
            if state.candidates.contains_key(id) {
                state.unsafe_locals.insert(*id);
            }
        }
        Expr::Closure { captures, body, .. } => {
            for captured in captures {
                if state.candidates.contains_key(captured) {
                    state.unsafe_locals.insert(*captured);
                }
            }
            perry_hir::walker::walk_expr_children(expr, &mut |child| scan_expr(child, state));
            scan_stmts(body, state);
        }
        _ => {
            perry_hir::walker::walk_expr_children(expr, &mut |child| scan_expr(child, state));
        }
    }
}

fn mark_candidate_refs_unsafe(expr: &Expr, state: &mut DirectMethodUseState<'_>) {
    match expr {
        Expr::LocalGet(local_id) | Expr::LocalSet(local_id, _) => {
            if state.candidates.contains_key(local_id) {
                state.unsafe_locals.insert(*local_id);
            }
        }
        Expr::Update { id, .. } => {
            if state.candidates.contains_key(id) {
                state.unsafe_locals.insert(*id);
            }
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        mark_candidate_refs_unsafe(child, state)
    });
}

fn candidate_method_receiver<'a>(
    callee: &'a Expr,
    candidates: &HashMap<u32, String>,
) -> Option<(u32, &'a str)> {
    let Expr::PropertyGet { object, property } = callee else {
        return None;
    };
    let Expr::LocalGet(local_id) = object.as_ref() else {
        return None;
    };
    candidates
        .contains_key(local_id)
        .then_some((*local_id, property.as_str()))
}

fn class_allows_direct_methods(
    class_name: &str,
    methods: &HashSet<String>,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    let Some(class) = classes.get(class_name) else {
        return false;
    };
    let field_names = inherited_field_names(class, classes);
    if methods.iter().any(|method| field_names.contains(method)) {
        return false;
    }
    if super::this_as_value::class_uses_this_as_value(class, classes) {
        return false;
    }
    for method in methods {
        if super::is_class_getter(classes, class_name, method)
            || super::is_class_setter(classes, class_name, method)
        {
            return false;
        }
        let Some(method_fn) = resolve_instance_method(class_name, method, classes) else {
            return false;
        };
        if super::this_as_value::stmts_use_this_as_value(&method_fn.body, &field_names) {
            return false;
        }
    }
    true
}

fn class_allows_direct_fields(
    class_name: &str,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    let Some(class) = classes.get(class_name) else {
        return false;
    };
    !super::this_as_value::class_uses_this_as_value(class, classes)
}

fn class_field_allows_direct_access(
    class_name: &str,
    field: &str,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    let Some(ty) = class_field_declared_type(classes, class_name, field) else {
        return false;
    };
    if !matches!(ty, perry_types::Type::Number) {
        return true;
    }
    class_raw_f64_field_has_numeric_initial_value(class_name, field, classes)
}

fn class_field_is_raw_f64(
    classes: &HashMap<String, &perry_hir::Class>,
    class_name: &str,
    field: &str,
) -> bool {
    class_field_declared_type(classes, class_name, field)
        .is_some_and(|ty| matches!(ty, perry_types::Type::Number))
}

fn class_field_declared_type<'a>(
    classes: &'a HashMap<String, &perry_hir::Class>,
    class_name: &str,
    field: &str,
) -> Option<&'a perry_types::Type> {
    let mut current = Some(class_name);
    while let Some(name) = current {
        let class = classes.get(name)?;
        if let Some(field) = class
            .fields
            .iter()
            .find(|candidate| candidate.key_expr.is_none() && candidate.name == field)
        {
            return Some(&field.ty);
        }
        current = class.extends_name.as_deref();
    }
    None
}

fn class_raw_f64_field_has_numeric_initial_value(
    class_name: &str,
    field: &str,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    let Some(class) = classes.get(class_name) else {
        return false;
    };
    if let Some(field_decl) = class
        .fields
        .iter()
        .find(|candidate| candidate.key_expr.is_none() && candidate.name == field)
    {
        if let Some(init) = &field_decl.init {
            return expr_is_numeric_for_class_field(init, class_name, classes);
        }
    }
    constructor_unconditionally_sets_numeric_field(class, class_name, field, classes)
}

fn constructor_unconditionally_sets_numeric_field(
    class: &perry_hir::Class,
    class_name: &str,
    field: &str,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    let Some(ctor) = &class.constructor else {
        return false;
    };
    let mut saw_numeric_set = false;
    for stmt in &ctor.body {
        let Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) = stmt
        else {
            return false;
        };
        if !matches!(object.as_ref(), Expr::This) {
            return false;
        }
        if property == field {
            if !expr_is_numeric_for_class_field(value, class_name, classes) {
                return false;
            }
            saw_numeric_set = true;
        }
    }
    saw_numeric_set
}

fn expr_is_numeric_for_direct_field(
    expr: &Expr,
    target_field: &str,
    candidates: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                expr_is_numeric_for_direct_field(left, target_field, candidates, classes)
                    && expr_is_numeric_for_direct_field(right, target_field, candidates, classes)
            }
            _ => false,
        },
        Expr::PropertyGet { object, property } => {
            if property != target_field {
                return false;
            }
            let Expr::LocalGet(local_id) = object.as_ref() else {
                return false;
            };
            let Some(class_name) = candidates.get(local_id) else {
                return false;
            };
            class_field_is_raw_f64(classes, class_name, property)
        }
        _ => expr_is_numeric_for_class_field(expr, "", classes),
    }
}

fn expr_is_numeric_for_class_field(
    expr: &Expr,
    class_name: &str,
    classes: &HashMap<String, &perry_hir::Class>,
) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                expr_is_numeric_for_class_field(left, class_name, classes)
                    && expr_is_numeric_for_class_field(right, class_name, classes)
            }
            _ => false,
        },
        Expr::PropertyGet { object, property } if matches!(object.as_ref(), Expr::This) => {
            !class_name.is_empty() && class_field_is_raw_f64(classes, class_name, property)
        }
        _ => false,
    }
}

fn inherited_field_names(
    class: &perry_hir::Class,
    classes: &HashMap<String, &perry_hir::Class>,
) -> HashSet<String> {
    let mut fields = HashSet::new();
    fields.extend(class.fields.iter().map(|field| field.name.clone()));
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        let Some(parent_class) = classes.get(parent_name) else {
            break;
        };
        fields.extend(parent_class.fields.iter().map(|field| field.name.clone()));
        parent = parent_class.extends_name.as_deref();
    }
    fields
}

fn resolve_instance_method<'a>(
    class_name: &str,
    method: &str,
    classes: &'a HashMap<String, &perry_hir::Class>,
) -> Option<&'a perry_hir::Function> {
    let mut current = Some(class_name);
    while let Some(name) = current {
        let class = classes.get(name)?;
        if let Some(method_fn) = class
            .methods
            .iter()
            .find(|candidate| candidate.name == method)
        {
            return Some(method_fn);
        }
        current = class.extends_name.as_deref();
    }
    None
}
