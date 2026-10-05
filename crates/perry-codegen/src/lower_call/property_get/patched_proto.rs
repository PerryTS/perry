//! #11394: a method the program writes onto a builtin prototype.
//!
//! `Array.prototype.push = f` leaves every direct lowering of `arr.push(x)`
//! wrong: the array arm calls `js_array_push_*`, the Map/Set arms call the
//! native collection method, and the universal dispatcher matches the NAME
//! against its native arms. None of them reads the prototype slot, so the
//! patch never runs.
//!
//! The whole-program pre-scan (`perry_hir::patched_builtins`) collects the
//! method names written onto `Array`/`Map`/`Set`/`Function.prototype`; the
//! driver hands them to codegen here before any module is lowered. A call of
//! one of those names skips the whole specialised chain and calls
//! `js_native_call_method_patched_proto`, which performs the property lookup
//! and runs a user function it finds, or dispatches exactly as before. Programs
//! that patch no builtin prototype have an empty set and never reach this arm.

use std::sync::RwLock;

use anyhow::Result;
use perry_hir::{CallArg, Expr};

use crate::expr::FnCtx;
use crate::rooting;
use crate::type_analysis::receiver_class_name;
use crate::types::{DOUBLE, I64, PTR};

/// The method names the program writes onto a builtin prototype, sorted.
/// `"*"` stands for a write whose key was not static.
///
/// Set once by the compile driver before any module codegen runs, and folded
/// into the object-cache key: the same module lowers differently depending on
/// what OTHER modules patch.
static PROGRAM_PATCHED_PROTO_METHODS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Record the program's patched builtin-prototype method names. See
/// [`PROGRAM_PATCHED_PROTO_METHODS`].
pub fn set_program_patched_proto_methods(names: Vec<String>) {
    *PROGRAM_PATCHED_PROTO_METHODS.write().unwrap() = names;
}

/// See [`set_program_patched_proto_methods`].
pub fn program_patched_proto_methods() -> Vec<String> {
    PROGRAM_PATCHED_PROTO_METHODS.read().unwrap().clone()
}

fn method_patched(property: &str) -> bool {
    let names = PROGRAM_PATCHED_PROTO_METHODS.read().unwrap();
    !names.is_empty() && names.iter().any(|n| n == property || n == "*")
}

/// Does `object.property(…)` need the lookup-first entry?
///
/// Only a receiver that is a VALUE — a local, an imported value, a property or
/// element read, a call result, a literal, a fresh collection, a function — can
/// be an array, Map/Set or function instance. Everything else keeps its own
/// lowering: a class reference (`Sub.from(…)` is a static call), a global
/// namespace, a native module, an imported function or namespace, and `this`;
/// so does a known user class instance, whose methods the class-dispatch arms
/// resolve.
pub(crate) fn guards(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    if !method_patched(property) {
        return false;
    }
    let value_receiver = match object {
        Expr::ExternFuncRef { name, .. } => ctx.imported_vars.contains(name.as_str()),
        _ => matches!(
            object,
            Expr::LocalGet(_)
                | Expr::PropertyGet { .. }
                | Expr::IndexGet { .. }
                | Expr::Call { .. }
                | Expr::CallSpread { .. }
                | Expr::Array(_)
                | Expr::ArraySpread(_)
                | Expr::MapNew
                | Expr::MapNewFromArray(_)
                | Expr::SetNew
                | Expr::SetNewFromArray(_)
                | Expr::FuncRef(_)
                | Expr::Closure { .. }
        ),
    };
    value_receiver && receiver_class_name(ctx, object).is_none()
}

/// `@<bytes>` and length of the method name's interned UTF-8 bytes.
fn method_name_bytes(ctx: &mut FnCtx<'_>, property: &str) -> (String, String) {
    let idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(idx);
    (
        format!("@{}", entry.bytes_global),
        entry.byte_len.to_string(),
    )
}

/// Lower `object.property(args…)` to `js_native_call_method_patched_proto`.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<String> {
    // #11910: the prototype lookup runs before arguments that can observe it.
    if crate::expr::method_site::args_may_observe_lookup(ctx, args) {
        use crate::lower_call::lookup_first::{lower, Args, Key};
        return lower(
            ctx,
            object,
            Key::Name(property),
            Args::List(args),
            |ctx, p| {
                let (name_global, name_len) = method_name_bytes(ctx, property);
                crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
                ctx.block().call(
                    DOUBLE,
                    "js_native_call_method_patched_proto",
                    &[
                        (DOUBLE, &p.recv),
                        (PTR, &name_global),
                        (I64, &name_len),
                        (PTR, &p.args_ptr),
                        (I64, &p.argc),
                    ],
                )
            },
        );
    }
    let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
    operands.push(object);
    operands.extend(args.iter());
    rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv, arg_vals) = values.split_first().expect("the receiver is operand 0");
        let (name_global, name_len) = method_name_bytes(ctx, property);
        // The alloca lives in the entry block (a loop-body alloca is a stack
        // adjustment that is never restored, #167).
        let (args_ptr, args_len) = if arg_vals.is_empty() {
            ("null".to_string(), "0".to_string())
        } else {
            let buf = ctx.func.alloca_entry_array(DOUBLE, arg_vals.len());
            let blk = ctx.block();
            for (i, value) in arg_vals.iter().enumerate() {
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                blk.store(DOUBLE, value, &slot);
            }
            (buf, arg_vals.len().to_string())
        };
        crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
        Ok(ctx.block().call(
            DOUBLE,
            "js_native_call_method_patched_proto",
            &[
                (DOUBLE, recv),
                (PTR, &name_global),
                (I64, &name_len),
                (PTR, &args_ptr),
                (I64, &args_len),
            ],
        ))
    })
}

/// Spread form, `object.property(...args)`: every regular and spread argument
/// is bundled into one array (as `js_native_call_method_apply` takes it).
pub(crate) fn lower_spread(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[CallArg],
) -> Result<String> {
    if crate::lower_call::lookup_first::spread_args_may_observe_lookup(ctx, args) {
        use crate::lower_call::lookup_first::{lower, Args, Key};
        return lower(
            ctx,
            object,
            Key::Name(property),
            Args::Spread(args),
            |ctx, p| {
                let (name_global, name_len) = method_name_bytes(ctx, property);
                ctx.block().call(
                    DOUBLE,
                    "js_native_call_method_patched_proto_apply",
                    &[
                        (DOUBLE, &p.recv),
                        (PTR, &name_global),
                        (I64, &name_len),
                        (I64, &p.array),
                    ],
                )
            },
        );
    }
    let recv_box = crate::expr::lower_expr(ctx, object)?;
    // Bundling the arguments allocates; the receiver is re-read from a root.
    let mut receiver_group = rooting::open_rooted_group(1);
    let receiver_root = receiver_group.adopt(ctx, object, &recv_box, true);
    let result = crate::expr::call_spread::bundle_args_rooted(ctx, args, false, |ctx, acc| {
        let recv_box = receiver_group.reread(ctx, receiver_root)?;
        let (name_global, name_len) = method_name_bytes(ctx, property);
        Ok(ctx.block().call(
            DOUBLE,
            "js_native_call_method_patched_proto_apply",
            &[
                (DOUBLE, &recv_box),
                (PTR, &name_global),
                (I64, &name_len),
                (I64, acc),
            ],
        ))
    });
    receiver_group.release(ctx);
    result
}
