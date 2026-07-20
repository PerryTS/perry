//! Internal typed calling-convention selection.
//!
//! This is intentionally conservative. It only opts in helpers whose HIR body is
//! straight-line typed SSA over supported numeric/boolean parameters and
//! locals. The generic JSValue/NaN-box ABI remains the public fallback for
//! every other call shape.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, CompareOp, Expr, Function, LogicalOp, Stmt, UnaryOp};
use perry_types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedFunctionTrampolineKind {
    F64,
    I32,
    I1,
    StringRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TypedParamRep {
    F64,
    I32,
    I1,
    StringRef,
}

impl TypedParamRep {
    pub(crate) fn llvm_ty(self) -> crate::types::LlvmType {
        match self {
            Self::F64 => crate::types::DOUBLE,
            Self::I32 => crate::types::I32,
            Self::I1 => crate::types::I1,
            Self::StringRef => crate::types::I64,
        }
    }

    pub(crate) fn guard_fn(self) -> &'static str {
        match self {
            Self::F64 => "js_typed_f64_arg_guard",
            Self::I32 => "js_typed_i32_arg_guard",
            Self::I1 => "js_typed_i1_arg_guard",
            Self::StringRef => "js_typed_string_arg_guard",
        }
    }

    pub(crate) fn unbox_fn(self) -> &'static str {
        match self {
            Self::F64 => "js_typed_f64_arg_to_raw",
            Self::I32 => "js_typed_i32_arg_to_raw",
            Self::I1 => "js_typed_i1_arg_to_raw",
            Self::StringRef => "js_typed_string_arg_to_raw",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::F64 => "f64",
            Self::I32 => "i32",
            Self::I1 => "i1",
            Self::StringRef => "string",
        }
    }
}

pub(crate) fn typed_param_rep_for_type(ty: &Type) -> Option<TypedParamRep> {
    if matches!(ty, Type::Int32) {
        Some(TypedParamRep::I32)
    } else if is_f64_type(ty) {
        Some(TypedParamRep::F64)
    } else if matches!(ty, Type::Boolean) {
        Some(TypedParamRep::I1)
    } else if is_string_type(ty) {
        Some(TypedParamRep::StringRef)
    } else {
        None
    }
}

pub(crate) fn typed_param_reps_for_params(
    params: &[perry_hir::Param],
) -> Option<Vec<TypedParamRep>> {
    params
        .iter()
        .map(|param| typed_param_rep_for_type(&param.ty))
        .collect()
}

pub(crate) fn typed_f64_closure_capture_reps(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<Vec<(u32, TypedParamRep)>> {
    typed_closure_capture_reps(expr, module_local_types)
}

pub(crate) fn typed_i1_closure_capture_reps(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<Vec<(u32, TypedParamRep)>> {
    typed_closure_capture_reps(expr, module_local_types)
}

fn typed_closure_capture_reps(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<Vec<(u32, TypedParamRep)>> {
    let Expr::Closure { captures, .. } = expr else {
        return None;
    };
    let mut reps = Vec::with_capacity(captures.len());
    for id in captures {
        let ty = module_local_types.get(id)?;
        let rep = typed_param_rep_for_type(ty)?;
        reps.push((*id, rep));
    }
    Some(reps)
}

pub(crate) fn typed_i32_closure_capture_reps(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<Vec<(u32, TypedParamRep)>> {
    typed_closure_capture_reps(expr, module_local_types)
}

pub(crate) fn typed_string_closure_capture_reps(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<Vec<(u32, TypedParamRep)>> {
    typed_closure_capture_reps(expr, module_local_types)
}

pub(crate) fn emit_typed_arg_guard(
    blk: &mut crate::block::LlBlock,
    rep: TypedParamRep,
    arg: &str,
) -> String {
    let raw = blk.call(
        crate::types::I32,
        rep.guard_fn(),
        &[(crate::types::DOUBLE, arg)],
    );
    blk.icmp_ne(crate::types::I32, &raw, "0")
}

pub(crate) fn emit_typed_arg_to_raw(
    blk: &mut crate::block::LlBlock,
    rep: TypedParamRep,
    arg: &str,
) -> String {
    match rep {
        TypedParamRep::F64 => blk.call(
            crate::types::DOUBLE,
            rep.unbox_fn(),
            &[(crate::types::DOUBLE, arg)],
        ),
        TypedParamRep::I32 => blk.call(
            crate::types::I32,
            rep.unbox_fn(),
            &[(crate::types::DOUBLE, arg)],
        ),
        TypedParamRep::I1 => {
            let raw_i32 = blk.call(
                crate::types::I32,
                rep.unbox_fn(),
                &[(crate::types::DOUBLE, arg)],
            );
            blk.icmp_ne(crate::types::I32, &raw_i32, "0")
        }
        TypedParamRep::StringRef => blk.call(
            crate::types::I64,
            rep.unbox_fn(),
            &[(crate::types::DOUBLE, arg)],
        ),
    }
}

pub(crate) fn typed_param_reps_match_args(
    ctx: &crate::expr::FnCtx<'_>,
    reps: &[TypedParamRep],
    args: &[Expr],
) -> bool {
    reps.len() == args.len()
        && args.iter().zip(reps.iter()).all(|(arg, rep)| match rep {
            TypedParamRep::F64 => crate::type_analysis::is_numeric_expr(ctx, arg),
            TypedParamRep::I32 => {
                matches!(
                    crate::type_analysis::static_type_of(ctx, arg),
                    Some(Type::Int32)
                ) || matches!(
                    arg,
                    Expr::Integer(n)
                        if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(n)
                )
            }
            TypedParamRep::I1 => crate::type_analysis::is_bool_expr(ctx, arg),
            TypedParamRep::StringRef => crate::type_analysis::is_definitely_string_expr(ctx, arg),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedCloneRejectionReason {
    NotClosure,
    AsyncOrGenerator,
    Captures,
    CapturesThis,
    CapturesNewTarget,
    ReturnTypeNotF64,
    ReturnTypeNotI32,
    ReturnTypeNotI1,
    ReturnTypeNotString,
    ParamNotF64,
    ParamNotI32,
    ParamNotI1,
    ParamNotString,
    ParamDefault,
    RestParam,
    ArgumentsObject,
    BodyNotSingleReturn,
    BodyNotStraightLineTyped,
    ReturnExprNotTypedF64Safe,
    ReturnExprNotTypedI32Safe,
    ReturnExprNotTypedI1Safe,
    ReturnExprNotTypedStringSafe,
    I64Specialized,
    NoReceiverField,
    ReceiverClassExtends,
    ReceiverClassHasAccessor,
    ReceiverClassHasComputedMember,
    ReceiverClassHasComputedField,
    ReceiverFieldNotOwn,
    ReceiverFieldNotF64,
    ThisEscape,
}

impl TypedCloneRejectionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotClosure => "not_closure",
            Self::AsyncOrGenerator => "async_or_generator",
            Self::Captures => "captures",
            Self::CapturesThis => "captures_this",
            Self::CapturesNewTarget => "captures_new_target",
            Self::ReturnTypeNotF64 => "return_type_not_f64",
            Self::ReturnTypeNotI32 => "return_type_not_i32",
            Self::ReturnTypeNotI1 => "return_type_not_i1",
            Self::ReturnTypeNotString => "return_type_not_string",
            Self::ParamNotF64 => "param_not_f64",
            Self::ParamNotI32 => "param_not_i32",
            Self::ParamNotI1 => "param_not_i1",
            Self::ParamNotString => "param_not_string",
            Self::ParamDefault => "param_default",
            Self::RestParam => "rest_param",
            Self::ArgumentsObject => "arguments_object",
            Self::BodyNotSingleReturn => "body_not_single_return",
            Self::BodyNotStraightLineTyped => "body_not_straight_line_typed",
            Self::ReturnExprNotTypedF64Safe => "return_expr_not_typed_f64_safe",
            Self::ReturnExprNotTypedI32Safe => "return_expr_not_typed_i32_safe",
            Self::ReturnExprNotTypedI1Safe => "return_expr_not_typed_i1_safe",
            Self::ReturnExprNotTypedStringSafe => "return_expr_not_typed_string_safe",
            Self::I64Specialized => "i64_specialized",
            Self::NoReceiverField => "no_receiver_field",
            Self::ReceiverClassExtends => "receiver_class_extends",
            Self::ReceiverClassHasAccessor => "receiver_class_has_accessor",
            Self::ReceiverClassHasComputedMember => "receiver_class_has_computed_member",
            Self::ReceiverClassHasComputedField => "receiver_class_has_computed_field",
            Self::ReceiverFieldNotOwn => "receiver_field_not_own",
            Self::ReceiverFieldNotF64 => "receiver_field_not_f64",
            Self::ThisEscape => "this_escape",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedReceiverField {
    pub(crate) name: String,
    pub(crate) index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedReceiverMethodInfo {
    pub(crate) fields: Vec<TypedReceiverField>,
}

impl TypedReceiverMethodInfo {
    pub(crate) fn field_index(&self, name: &str) -> Option<u32> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.index)
    }
}

pub(crate) fn generic_function_body_name(generic_name: &str) -> String {
    format!("{generic_name}__generic")
}

pub(crate) fn generic_method_body_name(generic_name: &str) -> String {
    format!("{generic_name}__generic")
}

pub(crate) fn generic_closure_body_name(generic_name: &str) -> String {
    format!("{generic_name}__generic")
}

pub(crate) fn typed_f64_function_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_f64")
}

pub(crate) fn typed_i32_function_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i32")
}

pub(crate) fn typed_i1_function_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i1")
}

pub(crate) fn typed_string_function_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_string")
}

pub(crate) fn typed_f64_method_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_f64")
}

pub(crate) fn typed_f64_receiver_method_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_f64_recv")
}

pub(crate) fn typed_i1_method_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i1")
}

pub(crate) fn typed_i32_method_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i32")
}

pub(crate) fn typed_string_method_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_string")
}

pub(crate) fn typed_f64_closure_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_f64")
}

pub(crate) fn typed_i1_closure_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i1")
}

pub(crate) fn typed_i32_closure_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_i32")
}

pub(crate) fn typed_string_closure_name(generic_name: &str) -> String {
    format!("{generic_name}__typed_string")
}

#[allow(dead_code)]
pub(crate) fn is_typed_f64_function_candidate(function: &Function) -> bool {
    typed_f64_callable_rejection_reason(function).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_i32_function_candidate(function: &Function) -> bool {
    typed_i32_function_rejection_reason(function).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_i1_function_candidate(function: &Function) -> bool {
    typed_i1_function_rejection_reason_impl(function).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_string_function_candidate(function: &Function) -> bool {
    typed_string_function_rejection_reason(function).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_f64_method_candidate(method: &Function) -> bool {
    typed_f64_callable_rejection_reason(method).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_i1_method_candidate(method: &Function) -> bool {
    typed_i1_function_rejection_reason_impl(method).is_none()
}

#[allow(dead_code)]
pub(crate) fn is_typed_string_method_candidate(method: &Function) -> bool {
    typed_string_method_rejection_reason(method).is_none()
}

pub(crate) fn typed_f64_function_rejection_reason(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_f64_callable_rejection_reason(function)
}

pub(crate) fn typed_i32_function_rejection_reason(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_i32_function_rejection_reason_impl(function)
}

pub(crate) fn typed_i1_function_rejection_reason(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_i1_function_rejection_reason_impl(function)
}

pub(crate) fn typed_string_function_rejection_reason(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    if function.is_async || function.is_generator || function.was_plain_async {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if !function.captures.is_empty() {
        return Some(TypedCloneRejectionReason::Captures);
    }
    if !is_string_type(&function.return_type) {
        return Some(TypedCloneRejectionReason::ReturnTypeNotString);
    }

    let mut locals = HashSet::new();
    for param in &function.params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotString);
        };
        if matches!(rep, TypedParamRep::StringRef) {
            locals.insert(param.id);
        }
    }

    typed_string_body_rejection_reason(&function.body, locals)
}

pub(crate) fn typed_f64_method_rejection_reason(
    method: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_f64_callable_rejection_reason(method)
}

pub(crate) fn typed_f64_receiver_method_rejection_reason(
    class: &perry_hir::Class,
    method: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_f64_receiver_method_candidate(class, method).err()
}

pub(crate) fn typed_f64_receiver_method_info(
    class: &perry_hir::Class,
    method: &Function,
) -> Option<TypedReceiverMethodInfo> {
    typed_f64_receiver_method_candidate(class, method).ok()
}

pub(crate) fn typed_i1_method_rejection_reason(
    method: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_i1_function_rejection_reason_impl(method)
}

pub(crate) fn typed_i32_method_rejection_reason(
    method: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_i32_function_rejection_reason_impl(method)
}

pub(crate) fn typed_string_method_rejection_reason(
    method: &Function,
) -> Option<TypedCloneRejectionReason> {
    typed_string_function_rejection_reason(method)
}

#[allow(dead_code)]
pub(crate) fn is_typed_f64_closure_candidate(expr: &Expr) -> bool {
    typed_f64_closure_rejection_reason(expr).is_none()
}

pub(crate) fn typed_f64_closure_rejection_reason(expr: &Expr) -> Option<TypedCloneRejectionReason> {
    typed_f64_closure_rejection_reason_with_types(expr, &HashMap::new())
}

pub(crate) fn typed_f64_closure_rejection_reason_with_types(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<TypedCloneRejectionReason> {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        is_async,
        is_generator,
        ..
    } = expr
    else {
        return Some(TypedCloneRejectionReason::NotClosure);
    };
    if *is_async || *is_generator {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if *captures_this {
        return Some(TypedCloneRejectionReason::CapturesThis);
    }
    if *captures_new_target {
        return Some(TypedCloneRejectionReason::CapturesNewTarget);
    }
    if !mutable_captures.is_empty() || captures.iter().any(|id| mutable_captures.contains(id)) {
        return Some(TypedCloneRejectionReason::Captures);
    }

    let mut numeric_params = HashMap::new();
    for param in params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotF64);
        };
        numeric_params.insert(param.id, rep);
    }
    let Some(capture_reps) = typed_f64_closure_capture_reps(expr, module_local_types) else {
        return Some(TypedCloneRejectionReason::Captures);
    };
    for (capture_id, rep) in capture_reps {
        numeric_params.insert(capture_id, rep);
    }

    typed_f64_body_rejection_reason(body, numeric_params)
}

#[allow(dead_code)]
pub(crate) fn is_typed_i1_closure_candidate(expr: &Expr) -> bool {
    typed_i1_closure_rejection_reason(expr).is_none()
}

pub(crate) fn typed_i1_closure_rejection_reason(expr: &Expr) -> Option<TypedCloneRejectionReason> {
    typed_i1_closure_rejection_reason_with_types(expr, &HashMap::new())
}

pub(crate) fn typed_i1_closure_rejection_reason_with_types(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<TypedCloneRejectionReason> {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        is_async,
        is_generator,
        ..
    } = expr
    else {
        return Some(TypedCloneRejectionReason::NotClosure);
    };
    if *is_async || *is_generator {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if *captures_this {
        return Some(TypedCloneRejectionReason::CapturesThis);
    }
    if *captures_new_target {
        return Some(TypedCloneRejectionReason::CapturesNewTarget);
    }
    if captures.iter().any(|id| mutable_captures.contains(id)) {
        return Some(TypedCloneRejectionReason::Captures);
    }

    let mut locals = HashMap::new();
    for param in params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotI1);
        };
        locals.insert(param.id, rep);
    }
    let Some(capture_reps) = typed_i1_closure_capture_reps(expr, module_local_types) else {
        return Some(TypedCloneRejectionReason::Captures);
    };
    for (capture_id, rep) in capture_reps {
        locals.insert(capture_id, rep);
    }

    typed_i1_body_rejection_reason(body, locals)
}

pub(crate) fn typed_i32_closure_rejection_reason(expr: &Expr) -> Option<TypedCloneRejectionReason> {
    typed_i32_closure_rejection_reason_with_types(expr, &HashMap::new())
}

pub(crate) fn typed_i32_closure_rejection_reason_with_types(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<TypedCloneRejectionReason> {
    let Expr::Closure {
        params,
        return_type,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        is_async,
        is_generator,
        ..
    } = expr
    else {
        return Some(TypedCloneRejectionReason::NotClosure);
    };
    if *is_async || *is_generator {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if *captures_this {
        return Some(TypedCloneRejectionReason::CapturesThis);
    }
    if *captures_new_target {
        return Some(TypedCloneRejectionReason::CapturesNewTarget);
    }
    if !mutable_captures.is_empty() || captures.iter().any(|id| mutable_captures.contains(id)) {
        return Some(TypedCloneRejectionReason::Captures);
    }
    if !matches!(return_type, Type::Int32) {
        return Some(TypedCloneRejectionReason::ReturnTypeNotI32);
    }

    let mut locals = HashMap::new();
    for param in params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotI32);
        };
        locals.insert(param.id, rep);
    }
    let Some(capture_reps) = typed_i32_closure_capture_reps(expr, module_local_types) else {
        return Some(TypedCloneRejectionReason::Captures);
    };
    for (capture_id, rep) in capture_reps {
        locals.insert(capture_id, rep);
    }

    typed_i32_body_rejection_reason(body, locals)
}

#[allow(dead_code)]
pub(crate) fn is_typed_string_closure_candidate(expr: &Expr) -> bool {
    typed_string_closure_rejection_reason(expr).is_none()
}

pub(crate) fn typed_string_closure_rejection_reason(
    expr: &Expr,
) -> Option<TypedCloneRejectionReason> {
    typed_string_closure_rejection_reason_with_types(expr, &HashMap::new())
}

pub(crate) fn typed_string_closure_rejection_reason_with_types(
    expr: &Expr,
    module_local_types: &HashMap<u32, Type>,
) -> Option<TypedCloneRejectionReason> {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        is_async,
        is_generator,
        ..
    } = expr
    else {
        return Some(TypedCloneRejectionReason::NotClosure);
    };
    if *is_async || *is_generator {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if *captures_this {
        return Some(TypedCloneRejectionReason::CapturesThis);
    }
    if *captures_new_target {
        return Some(TypedCloneRejectionReason::CapturesNewTarget);
    }
    if captures.iter().any(|id| mutable_captures.contains(id)) {
        return Some(TypedCloneRejectionReason::Captures);
    }

    let mut locals = HashSet::new();
    for param in params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotString);
        };
        if matches!(rep, TypedParamRep::StringRef) {
            locals.insert(param.id);
        }
    }
    let Some(capture_reps) = typed_string_closure_capture_reps(expr, module_local_types) else {
        return Some(TypedCloneRejectionReason::Captures);
    };
    for (capture_id, rep) in capture_reps {
        if matches!(rep, TypedParamRep::StringRef) {
            locals.insert(capture_id);
        }
    }

    typed_string_body_rejection_reason(body, locals)
}

fn typed_i1_function_rejection_reason_impl(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    if function.is_async || function.is_generator || function.was_plain_async {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if !function.captures.is_empty() {
        return Some(TypedCloneRejectionReason::Captures);
    }
    if !matches!(function.return_type, Type::Boolean) {
        return Some(TypedCloneRejectionReason::ReturnTypeNotI1);
    }

    let mut locals = HashMap::new();
    for param in &function.params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotI1);
        };
        locals.insert(param.id, rep);
    }

    typed_i1_body_rejection_reason(&function.body, locals)
}

fn typed_i32_function_rejection_reason_impl(
    function: &Function,
) -> Option<TypedCloneRejectionReason> {
    if function.is_async || function.is_generator || function.was_plain_async {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if !function.captures.is_empty() {
        return Some(TypedCloneRejectionReason::Captures);
    }
    if !matches!(function.return_type, Type::Int32) {
        return Some(TypedCloneRejectionReason::ReturnTypeNotI32);
    }

    let mut locals = HashMap::new();
    for param in &function.params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotI32);
        };
        locals.insert(param.id, rep);
    }

    typed_i32_body_rejection_reason(&function.body, locals)
}

fn typed_f64_callable_rejection_reason(function: &Function) -> Option<TypedCloneRejectionReason> {
    if function.is_async || function.is_generator || function.was_plain_async {
        return Some(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if !function.captures.is_empty() {
        return Some(TypedCloneRejectionReason::Captures);
    }
    if !is_f64_type(&function.return_type) {
        return Some(TypedCloneRejectionReason::ReturnTypeNotF64);
    }

    let mut numeric_params = HashMap::new();
    for param in &function.params {
        if param.default.is_some() {
            return Some(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Some(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Some(TypedCloneRejectionReason::ArgumentsObject);
        }
        let Some(rep) = typed_param_rep_for_type(&param.ty) else {
            return Some(TypedCloneRejectionReason::ParamNotF64);
        };
        numeric_params.insert(param.id, rep);
    }

    typed_f64_body_rejection_reason(&function.body, numeric_params)
}

fn is_f64_type(ty: &Type) -> bool {
    matches!(ty, Type::Number)
}

fn is_numeric_typed_type(ty: &Type) -> bool {
    matches!(ty, Type::Number | Type::Int32)
}

fn is_string_type(ty: &Type) -> bool {
    matches!(ty, Type::String | Type::StringLiteral(_))
}

fn typed_rep_for_declared_numeric_type(ty: &Type) -> Option<TypedParamRep> {
    match ty {
        Type::Number => Some(TypedParamRep::F64),
        Type::Int32 => Some(TypedParamRep::I32),
        _ => None,
    }
}

fn integer_literal_fits_i32(n: i64) -> bool {
    (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&n)
}

fn typed_receiver_own_field_index(
    class: &perry_hir::Class,
    property: &str,
) -> Result<u32, TypedCloneRejectionReason> {
    let mut index = 0u32;
    for field in &class.fields {
        if field.key_expr.is_some() {
            return Err(TypedCloneRejectionReason::ReceiverClassHasComputedField);
        }
        if field.name == property {
            if crate::typed_shape::type_is_raw_f64_candidate(&field.ty) {
                return Ok(index);
            }
            return Err(TypedCloneRejectionReason::ReceiverFieldNotF64);
        }
        index += 1;
    }
    Err(TypedCloneRejectionReason::ReceiverFieldNotOwn)
}

fn typed_f64_receiver_method_candidate(
    class: &perry_hir::Class,
    method: &Function,
) -> Result<TypedReceiverMethodInfo, TypedCloneRejectionReason> {
    if method.is_async || method.is_generator || method.was_plain_async {
        return Err(TypedCloneRejectionReason::AsyncOrGenerator);
    }
    if !method.captures.is_empty() {
        return Err(TypedCloneRejectionReason::Captures);
    }
    if !is_f64_type(&method.return_type) {
        return Err(TypedCloneRejectionReason::ReturnTypeNotF64);
    }
    // Keep this first slice exact: only methods on a final known receiver shape
    // with own string-keyed fields. Parent field offsets and inherited method
    // resolution remain on the generic ABI until the proof is widened.
    if class.extends_name.is_some() || class.extends.is_some() || class.extends_expr.is_some() {
        return Err(TypedCloneRejectionReason::ReceiverClassExtends);
    }
    if !class.getters.is_empty() || !class.setters.is_empty() {
        return Err(TypedCloneRejectionReason::ReceiverClassHasAccessor);
    }
    if !class.computed_members.is_empty() {
        return Err(TypedCloneRejectionReason::ReceiverClassHasComputedMember);
    }
    if class.fields.iter().any(|field| field.key_expr.is_some()) {
        return Err(TypedCloneRejectionReason::ReceiverClassHasComputedField);
    }

    let mut locals = HashMap::new();
    for param in &method.params {
        if param.default.is_some() {
            return Err(TypedCloneRejectionReason::ParamDefault);
        }
        if param.is_rest {
            return Err(TypedCloneRejectionReason::RestParam);
        }
        if param.arguments_object.is_some() {
            return Err(TypedCloneRejectionReason::ArgumentsObject);
        }
        if !is_f64_type(&param.ty) {
            return Err(TypedCloneRejectionReason::ParamNotF64);
        }
        locals.insert(param.id, TypedParamRep::F64);
    }

    let mut used_fields = Vec::new();
    let mut used_field_names = HashSet::new();
    typed_f64_receiver_body_rejection_reason(
        class,
        &method.body,
        locals,
        &mut used_fields,
        &mut used_field_names,
    )?;
    if used_fields.is_empty() {
        return Err(TypedCloneRejectionReason::NoReceiverField);
    }
    Ok(TypedReceiverMethodInfo {
        fields: used_fields,
    })
}

fn typed_f64_receiver_body_rejection_reason(
    class: &perry_hir::Class,
    body: &[Stmt],
    mut locals: HashMap<u32, TypedParamRep>,
    used_fields: &mut Vec<TypedReceiverField>,
    used_field_names: &mut HashSet<String>,
) -> Result<(), TypedCloneRejectionReason> {
    let Some((last, prefix)) = body.split_last() else {
        return Err(TypedCloneRejectionReason::BodyNotSingleReturn);
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty)
                && receiver_expr_is_typed_f64_safe(
                    class,
                    expr,
                    &locals,
                    used_fields,
                    used_field_names,
                )
                .is_ok() =>
            {
                locals.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let { .. } => {
                return Err(TypedCloneRejectionReason::BodyNotStraightLineTyped);
            }
            _ => return Err(TypedCloneRejectionReason::BodyNotStraightLineTyped),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => {
            receiver_expr_is_typed_f64_safe(class, expr, &locals, used_fields, used_field_names)
                .map(|_| ())
                .map_err(|_| TypedCloneRejectionReason::ReturnExprNotTypedF64Safe)
        }
        _ => Err(TypedCloneRejectionReason::BodyNotSingleReturn),
    }
}

fn receiver_expr_is_typed_f64_safe(
    class: &perry_hir::Class,
    expr: &Expr,
    locals: &HashMap<u32, TypedParamRep>,
    used_fields: &mut Vec<TypedReceiverField>,
    used_field_names: &mut HashSet<String>,
) -> Result<(), TypedCloneRejectionReason> {
    match expr {
        Expr::Number(_) | Expr::Integer(_) => Ok(()),
        Expr::LocalGet(id) if matches!(locals.get(id), Some(TypedParamRep::F64)) => Ok(()),
        Expr::LocalGet(_) => Err(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe),
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            let index = typed_receiver_own_field_index(class, property)?;
            if used_field_names.insert(property.clone()) {
                used_fields.push(TypedReceiverField {
                    name: property.clone(),
                    index,
                });
            }
            Ok(())
        }
        Expr::This => Err(TypedCloneRejectionReason::ThisEscape),
        Expr::Unary { op, operand } => {
            if matches!(op, UnaryOp::Pos | UnaryOp::Neg) {
                receiver_expr_is_typed_f64_safe(
                    class,
                    operand,
                    locals,
                    used_fields,
                    used_field_names,
                )
            } else {
                Err(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe)
            }
        }
        Expr::Binary { op, left, right } => {
            if !matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
            ) {
                return Err(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe);
            }
            receiver_expr_is_typed_f64_safe(class, left, locals, used_fields, used_field_names)?;
            receiver_expr_is_typed_f64_safe(class, right, locals, used_fields, used_field_names)
        }
        _ => Err(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe),
    }
}

fn typed_f64_body_rejection_reason(
    body: &[Stmt],
    mut locals: HashMap<u32, TypedParamRep>,
) -> Option<TypedCloneRejectionReason> {
    let Some((last, prefix)) = body.split_last() else {
        return Some(TypedCloneRejectionReason::BodyNotSingleReturn);
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) && expr_is_typed_f64_safe(expr, &locals) => {
                locals.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } if expr_is_typed_i32_safe(expr, &locals) => {
                locals.insert(*id, TypedParamRep::I32);
            }
            _ => return Some(TypedCloneRejectionReason::BodyNotStraightLineTyped),
        }
    }
    match last {
        Stmt::Return(Some(expr)) if expr_is_typed_f64_safe(expr, &locals) => None,
        Stmt::Return(Some(_)) => Some(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe),
        _ => Some(TypedCloneRejectionReason::BodyNotSingleReturn),
    }
}

fn typed_i32_body_rejection_reason(
    body: &[Stmt],
    mut locals: HashMap<u32, TypedParamRep>,
) -> Option<TypedCloneRejectionReason> {
    let Some((last, prefix)) = body.split_last() else {
        return Some(TypedCloneRejectionReason::BodyNotSingleReturn);
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } if expr_is_typed_i32_safe(expr, &locals) => {
                locals.insert(*id, TypedParamRep::I32);
            }
            _ => return Some(TypedCloneRejectionReason::BodyNotStraightLineTyped),
        }
    }
    match last {
        Stmt::Return(Some(expr)) if expr_is_typed_i32_safe(expr, &locals) => None,
        Stmt::Return(Some(_)) => Some(TypedCloneRejectionReason::ReturnExprNotTypedI32Safe),
        _ => Some(TypedCloneRejectionReason::BodyNotSingleReturn),
    }
}

fn typed_i1_body_rejection_reason(
    body: &[Stmt],
    mut locals: HashMap<u32, TypedParamRep>,
) -> Option<TypedCloneRejectionReason> {
    let Some((last, prefix)) = body.split_last() else {
        return Some(TypedCloneRejectionReason::BodyNotSingleReturn);
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Boolean,
                mutable: false,
                init: Some(expr),
                ..
            } if expr_is_typed_i1_safe(expr, &locals) => {
                locals.insert(*id, TypedParamRep::I1);
            }
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_numeric_typed_type(ty) && expr_is_typed_f64_safe(expr, &locals) => {
                locals.insert(*id, typed_rep_for_declared_numeric_type(ty).unwrap());
            }
            _ => return Some(TypedCloneRejectionReason::BodyNotStraightLineTyped),
        }
    }
    match last {
        Stmt::Return(Some(expr)) if expr_is_typed_i1_safe(expr, &locals) => None,
        Stmt::Return(Some(_)) => Some(TypedCloneRejectionReason::ReturnExprNotTypedI1Safe),
        _ => Some(TypedCloneRejectionReason::BodyNotSingleReturn),
    }
}

fn typed_string_body_rejection_reason(
    body: &[Stmt],
    mut locals: HashSet<u32>,
) -> Option<TypedCloneRejectionReason> {
    let Some((last, prefix)) = body.split_last() else {
        return Some(TypedCloneRejectionReason::BodyNotSingleReturn);
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_string_type(ty) && expr_is_typed_string_safe(expr, &locals) => {
                locals.insert(*id);
            }
            _ => return Some(TypedCloneRejectionReason::BodyNotStraightLineTyped),
        }
    }
    match last {
        Stmt::Return(Some(expr)) if expr_is_typed_string_safe(expr, &locals) => None,
        Stmt::Return(Some(_)) => Some(TypedCloneRejectionReason::ReturnExprNotTypedStringSafe),
        _ => Some(TypedCloneRejectionReason::BodyNotSingleReturn),
    }
}

fn expr_is_typed_f64_safe(expr: &Expr, locals: &HashMap<u32, TypedParamRep>) -> bool {
    match expr {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) => matches!(
            locals.get(id),
            Some(TypedParamRep::F64 | TypedParamRep::I32)
        ),
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Pos | UnaryOp::Neg) && expr_is_typed_f64_safe(operand, locals)
        }
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
            ) && expr_is_typed_f64_safe(left, locals)
                && expr_is_typed_f64_safe(right, locals)
        }
        _ => false,
    }
}

fn expr_is_typed_i32_safe(expr: &Expr, locals: &HashMap<u32, TypedParamRep>) -> bool {
    match expr {
        Expr::Integer(n) => integer_literal_fits_i32(*n),
        Expr::LocalGet(id) => matches!(locals.get(id), Some(TypedParamRep::I32)),
        Expr::Unary {
            op: UnaryOp::BitNot,
            operand,
        } => expr_is_typed_i32_safe(operand, locals),
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
            ) && expr_is_typed_i32_safe(left, locals)
                && expr_is_typed_i32_safe(right, locals)
        }
        _ => false,
    }
}

fn expr_is_typed_i1_safe(expr: &Expr, locals: &HashMap<u32, TypedParamRep>) -> bool {
    match expr {
        Expr::Bool(_) => true,
        Expr::LocalGet(id) => matches!(locals.get(id), Some(TypedParamRep::I1)),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => expr_is_typed_i1_safe(operand, locals),
        Expr::Logical { op, left, right } => {
            matches!(op, LogicalOp::And | LogicalOp::Or)
                && expr_is_typed_i1_safe(left, locals)
                && expr_is_typed_i1_safe(right, locals)
        }
        Expr::Compare { op, left, right } => {
            let bool_compare = matches!(
                op,
                CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
            ) && expr_is_typed_i1_safe(left, locals)
                && expr_is_typed_i1_safe(right, locals);
            let numeric_compare =
                expr_is_typed_f64_safe(left, locals) && expr_is_typed_f64_safe(right, locals);
            bool_compare || numeric_compare
        }
        _ => false,
    }
}

fn expr_is_typed_string_safe(expr: &Expr, locals: &HashSet<u32>) -> bool {
    match expr {
        Expr::LocalGet(id) => locals.contains(id),
        _ => false,
    }
}

fn lower_typed_f64_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    reps: &HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Number(n) => Ok(crate::nanbox::double_literal(*n)),
        Expr::Integer(n) => Ok(format!("{}.0", *n)),
        Expr::LocalGet(id) => {
            let value = locals
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("%arg{id}"));
            if matches!(reps.get(id), Some(TypedParamRep::I32)) {
                Ok(blk.sitofp(crate::types::I32, &value, crate::types::DOUBLE))
            } else {
                Ok(value)
            }
        }
        Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        } => lower_typed_f64_expr_with_env(blk, operand, locals, reps),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let v = lower_typed_f64_expr_with_env(blk, operand, locals, reps)?;
            Ok(blk.fneg(&v))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_typed_f64_expr_with_env(blk, left, locals, reps)?;
            let r = lower_typed_f64_expr_with_env(blk, right, locals, reps)?;
            Ok(match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                _ => {
                    anyhow::bail!("typed-f64 clone cannot lower non-arithmetic expression")
                }
            })
        }
        _ => anyhow::bail!(
            "typed-f64 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_i32_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Integer(n) if integer_literal_fits_i32(*n) => Ok(n.to_string()),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::Unary {
            op: UnaryOp::BitNot,
            operand,
        } => {
            let v = lower_typed_i32_expr_with_env(blk, operand, locals)?;
            Ok(blk.xor(crate::types::I32, &v, "-1"))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_typed_i32_expr_with_env(blk, left, locals)?;
            let r_raw = lower_typed_i32_expr_with_env(blk, right, locals)?;
            let r = if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
                blk.and(crate::types::I32, &r_raw, "31")
            } else {
                r_raw
            };
            Ok(match op {
                BinaryOp::BitAnd => blk.and(crate::types::I32, &l, &r),
                BinaryOp::BitOr => blk.or(crate::types::I32, &l, &r),
                BinaryOp::BitXor => blk.xor(crate::types::I32, &l, &r),
                BinaryOp::Shl => blk.shl(crate::types::I32, &l, &r),
                BinaryOp::Shr => blk.ashr(crate::types::I32, &l, &r),
                _ => anyhow::bail!("typed-i32 clone cannot lower non-bitwise expression"),
            })
        }
        _ => anyhow::bail!(
            "typed-i32 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_i1_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    reps: &HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Bool(value) => Ok(value.to_string()),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => {
            let v = lower_typed_i1_expr_with_env(blk, operand, locals, reps)?;
            Ok(blk.xor(crate::types::I1, &v, "true"))
        }
        Expr::Logical { op, left, right } => {
            let l = lower_typed_i1_expr_with_env(blk, left, locals, reps)?;
            let r = lower_typed_i1_expr_with_env(blk, right, locals, reps)?;
            Ok(match op {
                LogicalOp::And => blk.and(crate::types::I1, &l, &r),
                LogicalOp::Or => blk.or(crate::types::I1, &l, &r),
                LogicalOp::Coalesce => {
                    anyhow::bail!("typed-i1 clone cannot lower nullish coalesce")
                }
            })
        }
        Expr::Compare { op, left, right } => {
            if expr_is_typed_i1_safe(left, reps)
                && expr_is_typed_i1_safe(right, reps)
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                )
            {
                let l = lower_typed_i1_expr_with_env(blk, left, locals, reps)?;
                let r = lower_typed_i1_expr_with_env(blk, right, locals, reps)?;
                return Ok(match op {
                    CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(crate::types::I1, &l, &r),
                    CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(crate::types::I1, &l, &r),
                    _ => unreachable!("guarded boolean comparison op"),
                });
            }
            if expr_is_typed_f64_safe(left, reps) && expr_is_typed_f64_safe(right, reps) {
                if expr_is_typed_i32_safe(left, reps) && expr_is_typed_i32_safe(right, reps) {
                    let l = lower_typed_i32_expr_with_env(blk, left, locals)?;
                    let r = lower_typed_i32_expr_with_env(blk, right, locals)?;
                    return Ok(match op {
                        CompareOp::Eq | CompareOp::LooseEq => {
                            blk.icmp_eq(crate::types::I32, &l, &r)
                        }
                        CompareOp::Ne | CompareOp::LooseNe => {
                            blk.icmp_ne(crate::types::I32, &l, &r)
                        }
                        CompareOp::Lt => blk.icmp_slt(crate::types::I32, &l, &r),
                        CompareOp::Le => blk.icmp_sle(crate::types::I32, &l, &r),
                        CompareOp::Gt => blk.icmp_sgt(crate::types::I32, &l, &r),
                        CompareOp::Ge => blk.icmp_sge(crate::types::I32, &l, &r),
                    });
                }
                let l = lower_typed_f64_expr_with_env(blk, left, locals, reps)?;
                let r = lower_typed_f64_expr_with_env(blk, right, locals, reps)?;
                let cond = match op {
                    CompareOp::Eq | CompareOp::LooseEq => "oeq",
                    CompareOp::Ne | CompareOp::LooseNe => "une",
                    CompareOp::Lt => "olt",
                    CompareOp::Le => "ole",
                    CompareOp::Gt => "ogt",
                    CompareOp::Ge => "oge",
                };
                return Ok(blk.fcmp(cond, &l, &r));
            }
            anyhow::bail!("typed-i1 clone cannot lower mixed comparison")
        }
        _ => anyhow::bail!(
            "typed-i1 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_string_expr_with_env(
    expr: &Expr,
    locals: &HashMap<u32, String>,
) -> anyhow::Result<String> {
    match expr {
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        _ => anyhow::bail!(
            "typed-string clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

pub(crate) fn lower_typed_f64_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    lower_typed_f64_body_with_seed_locals_and_reps(blk, params, body, locals, HashMap::new())
}

pub(crate) fn lower_typed_f64_body_with_seed_locals_and_reps(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
    mut reps: HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
        if let Some(rep) = typed_param_rep_for_type(&param.ty) {
            reps.insert(param.id, rep);
        }
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-f64 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I32);
            }
            _ => anyhow::bail!("typed-f64 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_f64_expr_with_env(blk, expr, &locals, &reps),
        _ => anyhow::bail!("typed-f64 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_f64_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_f64_body_with_seed_locals(blk, params, body, HashMap::new())
}

pub(crate) fn lower_typed_i32_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_i32_body_with_seed_locals(blk, params, body, HashMap::new())
}

pub(crate) fn lower_typed_i32_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-i32 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-i32 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_i32_expr_with_env(blk, expr, &locals),
        _ => anyhow::bail!("typed-i32 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_string_body_with_seed_locals(
    _blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-string clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_string_type(ty) => {
                let value = lower_typed_string_expr_with_env(expr, &locals)?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-string clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_string_expr_with_env(expr, &locals),
        _ => anyhow::bail!("typed-string clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_string_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_string_body_with_seed_locals(blk, params, body, HashMap::new())
}

fn lower_typed_f64_receiver_field(blk: &mut crate::block::LlBlock, field_index: u32) -> String {
    let obj_ptr = blk.inttoptr(crate::types::I64, "%this_obj");
    let fields_base = blk.gep(crate::types::I8, &obj_ptr, &[(crate::types::I64, "24")]);
    let field_index_str = field_index.to_string();
    let field_ptr = blk.gep(
        crate::types::DOUBLE,
        &fields_base,
        &[(crate::types::I64, &field_index_str)],
    );
    blk.load(crate::types::DOUBLE, &field_ptr)
}

fn lower_typed_f64_receiver_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    receiver: &TypedReceiverMethodInfo,
) -> anyhow::Result<String> {
    match expr {
        Expr::Number(n) => Ok(crate::nanbox::double_literal(*n)),
        Expr::Integer(n) => Ok(format!("{}.0", *n)),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            let Some(field_index) = receiver.field_index(property) else {
                anyhow::bail!("typed-f64 receiver clone cannot lower unproven receiver field")
            };
            Ok(lower_typed_f64_receiver_field(blk, field_index))
        }
        Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        } => lower_typed_f64_receiver_expr_with_env(blk, operand, locals, receiver),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let v = lower_typed_f64_receiver_expr_with_env(blk, operand, locals, receiver)?;
            Ok(blk.fneg(&v))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_typed_f64_receiver_expr_with_env(blk, left, locals, receiver)?;
            let r = lower_typed_f64_receiver_expr_with_env(blk, right, locals, receiver)?;
            Ok(match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                _ => {
                    anyhow::bail!("typed-f64 receiver clone cannot lower non-arithmetic expression")
                }
            })
        }
        _ => anyhow::bail!(
            "typed-f64 receiver clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

pub(crate) fn lower_typed_f64_receiver_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    receiver: &TypedReceiverMethodInfo,
) -> anyhow::Result<String> {
    let mut locals = HashMap::new();
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-f64 receiver clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_receiver_expr_with_env(blk, expr, &locals, receiver)?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-f64 receiver clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => {
            lower_typed_f64_receiver_expr_with_env(blk, expr, &locals, receiver)
        }
        _ => anyhow::bail!("typed-f64 receiver clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_i1_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
    mut reps: HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
        if let Some(rep) = typed_param_rep_for_type(&param.ty) {
            reps.insert(param.id, rep);
        }
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-i1 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Boolean,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i1_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I1);
            }
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I32);
            }
            _ => anyhow::bail!("typed-i1 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_i1_expr_with_env(blk, expr, &locals, &reps),
        _ => anyhow::bail!("typed-i1 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_i1_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_i1_body_with_seed_locals(blk, params, body, HashMap::new(), HashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::Param;

    fn param(id: u32, name: &str, ty: Type) -> Param {
        Param {
            id,
            name: name.to_string(),
            ty,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    fn function(return_type: Type, params: Vec<Param>, body: Vec<Stmt>) -> Function {
        Function {
            id: 1,
            name: "mixed".to_string(),
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn ret(expr: Expr) -> Vec<Stmt> {
        vec![Stmt::Return(Some(expr))]
    }

    #[test]
    fn f64_clone_accepts_mixed_raw_params_when_return_expr_is_numeric_safe() {
        let f = function(
            Type::Number,
            vec![
                param(10, "n", Type::Number),
                param(11, "i", Type::Int32),
                param(12, "flag", Type::Boolean),
            ],
            ret(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::LocalGet(11)),
            }),
        );

        assert_eq!(typed_f64_function_rejection_reason(&f), None);
        assert_eq!(
            typed_param_reps_for_params(&f.params),
            Some(vec![
                TypedParamRep::F64,
                TypedParamRep::I32,
                TypedParamRep::I1
            ])
        );
    }

    #[test]
    fn f64_clone_accepts_raw_i32_locals_before_numeric_return() {
        let f = function(
            Type::Number,
            vec![param(10, "n", Type::Number), param(11, "i", Type::Int32)],
            vec![
                Stmt::Let {
                    id: 12,
                    name: "mask".to_string(),
                    ty: Type::Int32,
                    mutable: false,
                    init: Some(Expr::Binary {
                        op: BinaryOp::BitOr,
                        left: Box::new(Expr::LocalGet(11)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                },
                Stmt::Return(Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(10)),
                    right: Box::new(Expr::LocalGet(12)),
                })),
            ],
        );

        assert_eq!(typed_f64_function_rejection_reason(&f), None);
    }

    #[test]
    fn f64_clone_rejects_unsafe_mixed_rep_use() {
        let f = function(
            Type::Number,
            vec![
                param(10, "n", Type::Number),
                param(11, "flag", Type::Boolean),
            ],
            ret(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::LocalGet(11)),
            }),
        );

        assert_eq!(
            typed_f64_function_rejection_reason(&f),
            Some(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe)
        );
    }

    #[test]
    fn string_clone_accepts_mixed_params_when_only_string_rep_flows_to_return() {
        let f = function(
            Type::String,
            vec![
                param(10, "s", Type::String),
                param(11, "i", Type::Int32),
                param(12, "flag", Type::Boolean),
            ],
            ret(Expr::LocalGet(10)),
        );

        assert_eq!(typed_string_function_rejection_reason(&f), None);
    }

    #[test]
    fn closure_clone_accepts_mixed_immutable_captures_for_numeric_return() {
        let expr = Expr::Closure {
            func_id: 7,
            params: vec![param(20, "scale", Type::Number)],
            return_type: Type::Number,
            body: ret(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(20)),
                right: Box::new(Expr::LocalGet(30)),
            }),
            captures: vec![30, 31],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        let module_local_types = HashMap::from([(30, Type::Int32), (31, Type::Boolean)]);

        assert_eq!(
            typed_f64_closure_rejection_reason_with_types(&expr, &module_local_types),
            None
        );
        assert_eq!(
            typed_f64_closure_capture_reps(&expr, &module_local_types),
            Some(vec![(30, TypedParamRep::I32), (31, TypedParamRep::I1)])
        );
    }
}
