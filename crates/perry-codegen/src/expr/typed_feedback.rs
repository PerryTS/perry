use crate::types::{DOUBLE, I32, I64, PTR};

use super::FnCtx;

#[derive(Clone, Copy)]
pub(crate) enum TypedFeedbackKind {
    PropertyGet,
    PropertySet,
    MethodCall,
    ClosureCall,
    ArrayElement,
    // #854: in-progress typed-feedback kinds; the guard/observe emit sites that
    // construct these are not wired into the codegen hot path yet.
    #[allow(dead_code)]
    NumericFieldWrite,
    #[allow(dead_code)]
    HelperReturn,
}

#[derive(Clone, Copy)]
pub(crate) struct TypedFeedbackContract {
    guard_name: &'static str,
    fallback_name: &'static str,
}

impl TypedFeedbackContract {
    pub(crate) const fn new(guard_name: &'static str, fallback_name: &'static str) -> Self {
        Self {
            guard_name,
            fallback_name,
        }
    }

    pub(crate) const fn object_get_by_name() -> Self {
        Self::new(
            "object_get_by_name_guard",
            "js_object_get_field_by_name_f64",
        )
    }

    pub(crate) const fn object_set_by_name() -> Self {
        Self::new("object_set_by_name_guard", "js_object_set_field_by_name")
    }

    pub(crate) const fn class_field_get() -> Self {
        Self::new("class_field_get_guard", "js_object_get_field_by_name_f64")
    }

    pub(crate) const fn class_field_set() -> Self {
        Self::new("class_field_set_guard", "js_object_set_field_by_name")
    }

    pub(crate) const fn method_call() -> Self {
        Self::new("method_call_guard", "js_native_call_method")
    }

    pub(crate) const fn method_direct_call() -> Self {
        Self::new("method_direct_call_guard", "js_native_call_method")
    }

    // #854: near-future typed-feedback contract seam, not yet emitted.
    #[allow(dead_code)]
    pub(crate) const fn method_apply_call() -> Self {
        Self::new("method_call_guard", "js_native_call_method_apply")
    }

    pub(crate) const fn closure_direct_call() -> Self {
        Self::new("closure_direct_call_guard", "js_closure_callN")
    }

    pub(crate) const fn array_get_index() -> Self {
        Self::new(
            "plain_array_index_get_guard",
            "js_typed_feedback_array_index_get_fallback_boxed",
        )
    }

    pub(crate) const fn numeric_array_get_index() -> Self {
        Self::new(
            "numeric_array_index_get_guard",
            "js_typed_feedback_array_index_get_fallback_boxed",
        )
    }

    pub(crate) const fn packed_f64_array_loop() -> Self {
        Self::new("packed_f64_array_loop_guard", "generic_jsvalue_loop")
    }

    pub(crate) const fn packed_i32_array_loop() -> Self {
        Self::new("packed_i32_array_loop_guard", "generic_jsvalue_loop")
    }

    pub(crate) const fn packed_u32_array_loop() -> Self {
        Self::new("packed_u32_array_loop_guard", "generic_jsvalue_loop")
    }

    pub(crate) const fn array_set_index() -> Self {
        Self::new("plain_array_index_set_guard", "js_array_set_f64_extend")
    }

    /// Runtime-key array store (`arr[k] = v` where `k` may be a string):
    /// the emitted edge is `js_typed_feedback_array_set_index_or_string`,
    /// so the contract must name that helper — not the numeric-only
    /// `js_array_set_f64_extend` — for the evidence packet to describe the
    /// dispatch actually taken.
    pub(crate) const fn array_set_index_or_string() -> Self {
        Self::new(
            "plain_array_index_set_guard",
            "js_typed_feedback_array_set_index_or_string",
        )
    }

    // #854: near-future typed-feedback contract seam, not yet emitted.
    #[allow(dead_code)]
    pub(crate) const fn bounded_array_set_index() -> Self {
        Self::new(
            "plain_array_index_set_guard",
            "js_typed_feedback_array_index_set_fallback_boxed",
        )
    }

    pub(crate) const fn numeric_array_set_index() -> Self {
        Self::new("numeric_array_index_set_guard", "js_array_set_f64_extend")
    }

    // #854: near-future typed-feedback contract seam, not yet emitted.
    #[allow(dead_code)]
    pub(crate) const fn bounded_numeric_array_set_index() -> Self {
        Self::new(
            "numeric_array_index_set_guard",
            "js_typed_feedback_array_index_set_fallback_boxed",
        )
    }

    pub(crate) const fn numeric_array_push() -> Self {
        Self::new("numeric_array_push_guard", "js_array_push_f64")
    }

    pub(crate) const fn array_set_string_key() -> Self {
        Self::new("array_string_key_set_guard", "js_array_set_string_key")
    }

    pub(crate) const fn polymorphic_index_set() -> Self {
        Self::new(
            "polymorphic_index_set_guard",
            "js_object_set_index_polymorphic",
        )
    }

    // #854: near-future typed-feedback contract seam, not yet emitted.
    #[allow(dead_code)]
    pub(crate) const fn unboxed_numeric_field_write() -> Self {
        Self::new(
            "unboxed_numeric_field_write_guard",
            "js_object_set_field_by_name",
        )
    }

    // #854: near-future typed-feedback contract seam, not yet emitted.
    #[allow(dead_code)]
    pub(crate) const fn helper_return() -> Self {
        Self::new("helper_return_shape_guard", "return_original_jsvalue")
    }
}

impl TypedFeedbackKind {
    fn raw(self) -> u32 {
        match self {
            Self::PropertyGet => 0,
            Self::PropertySet => 1,
            Self::MethodCall => 2,
            Self::ClosureCall => 3,
            Self::ArrayElement => 4,
            Self::NumericFieldWrite => 5,
            Self::HelperReturn => 6,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::PropertyGet => "property_get",
            Self::PropertySet => "property_set",
            Self::MethodCall => "method_call",
            Self::ClosureCall => "closure_call",
            Self::ArrayElement => "array_element",
            Self::NumericFieldWrite => "numeric_field_write",
            Self::HelperReturn => "helper_return",
        }
    }
}

fn escape_bytes_for_llvm_ir(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 8);
    s.push_str("c\"");
    for &b in bytes {
        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
            s.push(b as char);
        } else {
            s.push('\\');
            s.push_str(&format!("{:02X}", b));
        }
    }
    s.push_str("\\00\"");
    s
}

fn emit_typed_feedback_bytes_global(
    ctx: &mut FnCtx<'_>,
    site_local: u32,
    slot: &str,
    value: &str,
) -> String {
    let prefix = ctx.strings.module_prefix();
    let global = if prefix.is_empty() {
        format!("perry_typed_feedback_{}_{}", site_local, slot)
    } else {
        format!("perry_typed_feedback_{}__{}_{}", prefix, site_local, slot)
    };
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        global,
        value.len() + 1,
        escape_bytes_for_llvm_ir(value.as_bytes())
    ));
    format!("@{}", global)
}

/// Whether to emit the per-site `js_typed_feedback_register_site` call at all.
///
/// Typed feedback (#854) is an opt-in profiling feature, disabled at runtime
/// unless `PERRY_TYPED_FEEDBACK` / `PERRY_TYPED_FEEDBACK_TRACE` is set — in
/// which case `js_typed_feedback_register_site` early-returns and does nothing.
/// But the *call itself* (14 pointer/length arguments) was still emitted on
/// every property get/set, which on hot OOP code (e.g. a method doing
/// `this.x = this.x + 1` in a tight loop) costs two no-op cross-crate calls per
/// field access — the dominant cost of the `method_calls` benchmark (491× Node).
///
/// Gate emission on the same env that enables feedback at runtime: a normal
/// build (env unset) skips registration entirely and pays nothing; a profiling
/// build (`PERRY_TYPED_FEEDBACK=1 perry app.ts -o app && ./app`, env inherited
/// by the run) emits and uses it. The site-id is still allocated and returned
/// so the shape *guard* call is unchanged — guards stay correct either way.
fn typed_feedback_emission_enabled() -> bool {
    // Read fresh (not cached) so tests that toggle the env per-case observe the
    // change. At compile time this is a cheap getenv per property-access site.
    std::env::var_os("PERRY_TYPED_FEEDBACK").is_some()
        || std::env::var_os("PERRY_TYPED_FEEDBACK_TRACE").is_some()
}

pub(crate) fn emit_typed_feedback_register_site(
    ctx: &mut FnCtx<'_>,
    kind: TypedFeedbackKind,
    operation: &str,
    contract: TypedFeedbackContract,
) -> String {
    let local_site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let site_id = ctx.typed_feedback_site_id(local_site_id);
    // Default build: skip the no-op registration call (and its byte globals)
    // but keep the site-id stable for the guard call.
    if !typed_feedback_emission_enabled() {
        return site_id.to_string();
    }
    let module = if ctx.strings.module_prefix().is_empty() {
        "main".to_string()
    } else {
        ctx.strings.module_prefix().to_string()
    };
    let function = ctx.func.name.clone();
    let source_label = format!("{}:{}", kind.label(), operation);
    let module_global = emit_typed_feedback_bytes_global(ctx, local_site_id, "module", &module);
    let function_global =
        emit_typed_feedback_bytes_global(ctx, local_site_id, "function", &function);
    let source_global =
        emit_typed_feedback_bytes_global(ctx, local_site_id, "source", &source_label);
    let operation_global =
        emit_typed_feedback_bytes_global(ctx, local_site_id, "operation", operation);
    let guard_global =
        emit_typed_feedback_bytes_global(ctx, local_site_id, "guard", contract.guard_name);
    let fallback_global =
        emit_typed_feedback_bytes_global(ctx, local_site_id, "fallback", contract.fallback_name);
    let site_id_arg = site_id.to_string();
    let kind_arg = kind.raw().to_string();
    let module_len_arg = module.len().to_string();
    let function_len_arg = function.len().to_string();
    let source_len_arg = source_label.len().to_string();
    let operation_len_arg = operation.len().to_string();
    let guard_len_arg = contract.guard_name.len().to_string();
    let fallback_len_arg = contract.fallback_name.len().to_string();
    ctx.func.entry_setup_call_void(
        "js_typed_feedback_register_site",
        &[
            (I64, &site_id_arg),
            (I32, &kind_arg),
            (PTR, &module_global),
            (I64, &module_len_arg),
            (PTR, &function_global),
            (I64, &function_len_arg),
            (PTR, &source_global),
            (I64, &source_len_arg),
            (PTR, &operation_global),
            (I64, &operation_len_arg),
            (PTR, &guard_global),
            (I64, &guard_len_arg),
            (PTR, &fallback_global),
            (I64, &fallback_len_arg),
        ],
    );
    site_id.to_string()
}

// #854: near-future typed-feedback helper-return observation emitter; the call
// sites that invoke it are not wired into the codegen hot path yet.
#[allow(dead_code)]
pub(crate) fn emit_typed_feedback_observe_helper_return(
    ctx: &mut FnCtx<'_>,
    operation: &str,
    value: &str,
) -> String {
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::HelperReturn,
        operation,
        TypedFeedbackContract::helper_return(),
    );
    ctx.block().call(
        DOUBLE,
        "js_typed_feedback_observe_helper_return",
        &[(I64, &site_id), (DOUBLE, value)],
    )
}

pub(crate) fn native_region_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut pending_sep = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            pending_sep = false;
        } else {
            pending_sep = true;
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}
