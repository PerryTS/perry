//! Issue #92 Buffer numeric-read intrinsics.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `try_emit_buffer_read_intrinsic` and its
//! classification helper inline `buf.readInt32BE(offset)`-style reads
//! as LLVM load + bswap + `LoweredValue` instead of a runtime dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{access_facts_for_spec, BufferAccessSpec, FnCtx};
use crate::native_value::{BufferEndian, LoweredValue};
use crate::types::{F32, I32};

/// Issue #92: inline Buffer numeric reads (`buf.readInt32BE(offset)` etc.)
/// as LLVM load + bswap + convert instead of a runtime dispatch through
/// `js_native_call_method`. Called from the PropertyGet branch below when
/// the receiver is a Buffer / Uint8Array and the method name matches one
/// of the Node-style numeric read accessors. Returns `Ok(None)` when
/// intrinsification isn't possible (the generic path then catches it) —
/// currently that's any receiver that isn't a tracked `buffer_data_slot`.
struct BufferNumericReadSpec {
    width_bytes: u32,
    endian: BufferEndian,
    signed: bool,   // signed vs unsigned JS-number materialization
    is_float: bool, // true for readFloat*/readDouble*
}

fn classify_buffer_numeric_read(method: &str) -> Option<BufferNumericReadSpec> {
    use BufferNumericReadSpec as S;
    Some(match method {
        "readUInt8" | "readUint8" => S {
            width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            is_float: false,
        },
        "readInt8" => S {
            width_bytes: 1,
            endian: BufferEndian::Native,
            signed: true,
            is_float: false,
        },
        "readUInt16BE" | "readUint16BE" => S {
            width_bytes: 2,
            endian: BufferEndian::Big,
            signed: false,
            is_float: false,
        },
        "readUInt16LE" | "readUint16LE" => S {
            width_bytes: 2,
            endian: BufferEndian::Little,
            signed: false,
            is_float: false,
        },
        "readInt16BE" => S {
            width_bytes: 2,
            endian: BufferEndian::Big,
            signed: true,
            is_float: false,
        },
        "readInt16LE" => S {
            width_bytes: 2,
            endian: BufferEndian::Little,
            signed: true,
            is_float: false,
        },
        "readUInt32BE" | "readUint32BE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: false,
            is_float: false,
        },
        "readUInt32LE" | "readUint32LE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: false,
            is_float: false,
        },
        "readInt32BE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: true,
            is_float: false,
        },
        "readInt32LE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: true,
            is_float: false,
        },
        "readFloatBE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: false,
            is_float: true,
        },
        "readFloatLE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: false,
            is_float: true,
        },
        "readDoubleBE" => S {
            width_bytes: 8,
            endian: BufferEndian::Big,
            signed: false,
            is_float: true,
        },
        "readDoubleLE" => S {
            width_bytes: 8,
            endian: BufferEndian::Little,
            signed: false,
            is_float: true,
        },
        _ => return None,
    })
}

/// True for a Buffer numeric READ accessor name (`readUInt8`, `readInt32BE`,
/// …) — the method family `try_emit_buffer_read_intrinsic` inline-folds.
/// Shared with the whole-module shadow scan so the fold table and the deopt
/// decision stay in lockstep (one source of truth: `classify_buffer_numeric_read`).
pub(crate) fn is_buffer_numeric_read_method(name: &str) -> bool {
    classify_buffer_numeric_read(name).is_some()
}

/// Issue #6405 — whole-module pre-codegen scan: does this module assign to a
/// property whose name is a Buffer numeric read-method (`buf.readUInt8 = fn`,
/// `buf["readInt32BE"] = fn`)?
///
/// Node's Buffer IS an ordinary `Uint8Array`, so an own property SHADOWS the
/// same-named prototype method. Every dynamic dispatch path already honors
/// this (`dispatch_buffer_method` checks own props first), but a statically
/// provable `buf.readUInt8(0)` folds to the inline byte-load intrinsic below,
/// which reads the bytes directly and never consults the property table — so
/// the override was ignored. When any such assignment exists, the intrinsic
/// bails (returns `Ok(None)`) so the call routes through `js_native_call_method`
/// and the own-prop shadow wins. Zero runtime cost for the overwhelmingly
/// common program that never shadows a Buffer method — the fast path is
/// untouched there.
///
/// A per-module scan is sufficient: the intrinsic only fires on a buffer local
/// that `lower_buffer_access_proof` proves non-escaping (a closure-captured or
/// cross-module-shared/exported buffer is stamped hazardous and never folds),
/// so any shadow that can reach a folded read lives in this same module. Only
/// literal property names are matched; a dynamic `buf[computedName] = fn` is
/// out of scope (that shape already defeats the static buffer proof in
/// practice — see the issue).
pub(crate) fn module_shadows_buffer_read_method(module: &perry_hir::Module) -> bool {
    use perry_hir::{Expr, Stmt};

    fn expr_shadows(expr: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        match expr {
            Expr::PropertySet { property, .. } if is_buffer_numeric_read_method(property) => {
                *found = true;
                return;
            }
            Expr::PutValueSet { key, .. } => {
                if let Expr::String(name) = key.as_ref() {
                    if is_buffer_numeric_read_method(name) {
                        *found = true;
                        return;
                    }
                }
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| expr_shadows(child, found));
    }

    // Exhaustive on `Stmt` on purpose (no catch-all) — a new statement variant
    // that carries expressions must be threaded here, mirroring the walker's
    // enforced-exhaustiveness contract, or a shadow inside it slips through.
    fn stmt_shadows(stmt: &Stmt, found: &mut bool) {
        if *found {
            return;
        }
        match stmt {
            Stmt::Expr(e) | Stmt::Throw(e) => expr_shadows(e, found),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    expr_shadows(e, found);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    expr_shadows(e, found);
                }
            }
            Stmt::Labeled { body, .. } => stmt_shadows(body, found),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_shadows(condition, found);
                for s in then_branch {
                    stmt_shadows(s, found);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        stmt_shadows(s, found);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                expr_shadows(condition, found);
                for s in body {
                    stmt_shadows(s, found);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    stmt_shadows(i, found);
                }
                if let Some(c) = condition {
                    expr_shadows(c, found);
                }
                if let Some(u) = update {
                    expr_shadows(u, found);
                }
                for s in body {
                    stmt_shadows(s, found);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    stmt_shadows(s, found);
                }
                if let Some(catch_clause) = catch {
                    for s in &catch_clause.body {
                        stmt_shadows(s, found);
                    }
                }
                if let Some(finally_b) = finally {
                    for s in finally_b {
                        stmt_shadows(s, found);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr_shadows(discriminant, found);
                for case in cases {
                    if let Some(test) = &case.test {
                        expr_shadows(test, found);
                    }
                    for s in &case.body {
                        stmt_shadows(s, found);
                    }
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    fn scan_body(body: &[Stmt], found: &mut bool) {
        for s in body {
            stmt_shadows(s, found);
            if *found {
                return;
            }
        }
    }

    let mut found = false;
    // Top-level init, every function (which — post closure-conversion — also
    // carries the module's nested closures and object-literal methods), and
    // every class member body.
    scan_body(&module.init, &mut found);
    for func in &module.functions {
        if found {
            return true;
        }
        scan_body(&func.body, &mut found);
    }
    for class in &module.classes {
        if found {
            return true;
        }
        // A shadow assignment can hide in any expression position of a class,
        // not just member bodies: a dynamic `extends` expression, a field
        // initializer or computed field key, or a computed member key. Field
        // initializers in particular live in `fields[*].init` (emitted via
        // `apply_field_initializers_recursive`), NOT the constructor body, so
        // they need an explicit walk. (We do NOT match member *names* — a class
        // method/field named `readUInt8` is a user-class member, never an own
        // property on a `Buffer.alloc` local, so it can't shadow a folded read.)
        if let Some(ext) = &class.extends_expr {
            expr_shadows(ext, &mut found);
        }
        for f in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(key) = &f.key_expr {
                expr_shadows(key, &mut found);
            }
            if let Some(init) = &f.init {
                expr_shadows(init, &mut found);
            }
        }
        if let Some(ctor) = &class.constructor {
            scan_body(&ctor.body, &mut found);
        }
        for m in &class.methods {
            scan_body(&m.body, &mut found);
        }
        for m in &class.static_methods {
            scan_body(&m.body, &mut found);
        }
        for (_, g) in &class.getters {
            scan_body(&g.body, &mut found);
        }
        for (_, s) in &class.setters {
            scan_body(&s.body, &mut found);
        }
        for cm in &class.computed_members {
            expr_shadows(&cm.key_expr, &mut found);
            scan_body(&cm.function.body, &mut found);
        }
    }
    found
}

pub(super) fn try_emit_buffer_read_intrinsic(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method: &str,
    args: &[Expr],
) -> Result<Option<LoweredValue>> {
    // #6405: an own property shadows the same-named Buffer.prototype method.
    // If the module assigns any such method name as a property, deopt the
    // inline read fold so the call routes through the own-prop-aware runtime
    // dispatch. The flag is module-wide but only set for programs that
    // actually shadow, so the fast path is unaffected everywhere else.
    if ctx.program_shadows_buffer_read_method {
        return Ok(None);
    }
    let spec = match classify_buffer_numeric_read(method) {
        Some(s) => s,
        None => return Ok(None),
    };
    // Node-style readers take exactly one `offset` arg. `readUInt8(offset)`
    // allows omitted offset but the compiler sees that as 0-arg; not our
    // concern here — fall through to runtime which handles the default.
    if args.len() != 1 {
        return Ok(None);
    }
    let access_spec = BufferAccessSpec::buffer_numeric_read(
        spec.width_bytes,
        spec.endian,
        spec.signed,
        spec.is_float,
    );
    let Some(proof) = crate::expr::lower_buffer_access_proof(ctx, object, &args[0], access_spec)?
    else {
        return Ok(None);
    };
    let emission = crate::expr::emit_buffer_access_pointer(ctx, &proof, access_spec);
    let blk = ctx.block();
    // Load raw bytes at the correct width.
    let (load_ty, swap_intrinsic) = match spec.width_bytes {
        1 => ("i8", None),
        2 => ("i16", Some("llvm.bswap.i16")),
        4 => ("i32", Some("llvm.bswap.i32")),
        8 => ("i64", Some("llvm.bswap.i64")),
        _ => unreachable!(),
    };
    let raw = blk.fresh_reg();
    let load_align = if access_spec.index_unit == crate::native_value::BufferIndexUnit::Byte {
        1
    } else {
        spec.width_bytes.max(1)
    };
    blk.emit_raw(format!(
        "{} = load {}, ptr {}, align {}{}",
        raw, load_ty, emission.elem_ptr, load_align, emission.alias_metadata
    ));
    // Byte-swap for BE on multi-byte widths (swap.i8 doesn't exist; width=1
    // never has `swap=true` in the spec table anyway).
    let should_swap = spec.width_bytes > 1
        && spec.endian != BufferEndian::Native
        && spec.endian != target_endian();
    let swapped = match (should_swap, swap_intrinsic) {
        (true, Some(intr)) => {
            let r = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call {} @{}({} {})",
                r, load_ty, intr, load_ty, raw
            ));
            r
        }
        _ => raw,
    };
    let result = if spec.is_float {
        // Float/double: bitcast int bits → native float bits. readFloat*
        // stays region-local as f32; JS boundaries fpext it explicitly.
        let float_ty = if spec.width_bytes == 4 { F32 } else { "double" };
        let as_float = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = bitcast {} {} to {}",
            as_float, load_ty, swapped, float_ty
        ));
        if spec.width_bytes == 4 {
            LoweredValue::f32(as_float)
        } else {
            LoweredValue::f64(as_float)
        }
    } else {
        // Integer: keep the raw i32 in the native lattice. Signed reads
        // materialize with `sitofp`; unsigned reads materialize with
        // `uitofp`, including `readUInt32*` values whose high bit is set.
        let i32_val = match spec.width_bytes {
            1 | 2 => {
                if spec.signed {
                    blk.sext(load_ty, &swapped, I32)
                } else {
                    blk.zext(load_ty, &swapped, I32)
                }
            }
            4 => swapped,
            8 => {
                // Signed 8-byte reads (BigInt64) would need BigInt allocation;
                // only reach here for width_bytes==8 when is_float, which already
                // returned above. Defensive early-out.
                return Ok(None);
            }
            _ => unreachable!(),
        };
        if spec.signed {
            LoweredValue::i32(i32_val)
        } else {
            LoweredValue::u32(i32_val)
        }
    };
    let buffer_view = crate::expr::buffer_view_lowered_value(
        &emission.data_ptr,
        &emission.len_i32,
        proof.view.elem.clone(),
        proof.view.element_width_bytes,
        proof.view.index_unit,
        proof.view.view_byte_offset,
        proof.view.length_offset_from_data,
        proof.bounds.clone(),
        proof.alias.clone(),
    );
    let facts = access_facts_for_spec(access_spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        "BufferNumericRead",
        Some(proof.buffer_local_id),
        "BufferNumericRead.BufferView",
        &buffer_view,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("width_bytes={}", spec.width_bytes)],
    );
    let result_consumer = match result.rep.name() {
        "i32" => "BufferNumericRead.native_i32",
        "u32" => "BufferNumericRead.native_u32",
        "f32" => "BufferNumericRead.native_f32",
        "f64" => "BufferNumericRead.native_f64",
        _ => "BufferNumericRead.native_value",
    };
    let facts = access_facts_for_spec(access_spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        "BufferNumericRead",
        Some(proof.buffer_local_id),
        result_consumer,
        &result,
        Some(proof.bounds),
        Some(proof.alias),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        false,
        false,
        vec![
            format!("method={}", method),
            format!("width_bytes={}", spec.width_bytes),
            format!("endian={:?}", spec.endian),
        ],
    );
    Ok(Some(result))
}

fn target_endian() -> BufferEndian {
    if cfg!(target_endian = "big") {
        BufferEndian::Big
    } else {
        BufferEndian::Little
    }
}

#[cfg(test)]
mod shadow_scan_tests {
    use super::module_shadows_buffer_read_method;
    use perry_hir::{Class, ClassField, Expr, Module, Stmt};

    fn filler() -> Box<Expr> {
        Box::new(Expr::Integer(0))
    }

    /// `x[key] = v` (computed set with an explicit receiver — how
    /// `(b as any).readUInt8 = fn` and `b["readUInt8"] = fn` both lower).
    fn put_value_set_expr(key: &str) -> Expr {
        Expr::PutValueSet {
            target: filler(),
            key: Box::new(Expr::String(key.to_string())),
            value: filler(),
            receiver: filler(),
            strict: false,
        }
    }

    fn put_value_set(key: &str) -> Stmt {
        Stmt::Expr(put_value_set_expr(key))
    }

    /// A bare class carrying only the given instance fields — every other slot
    /// empty. Used to prove the scan walks field *initializers*, not just member
    /// bodies (field inits live in `fields[*].init`, emitted separately from the
    /// constructor body).
    fn class_with_fields(fields: Vec<ClassField>) -> Class {
        Class {
            id: 1,
            name: "C".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields,
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
            aliases: Vec::new(),
            is_nested: false,
        }
    }

    fn field_with_init(name: &str, init: Expr) -> ClassField {
        ClassField {
            name: name.to_string(),
            key_expr: None,
            ty: perry_types::Type::Any,
            init: Some(init),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }
    }

    /// `x.prop = v` (dot set).
    fn property_set(prop: &str) -> Stmt {
        Stmt::Expr(Expr::PropertySet {
            object: filler(),
            property: prop.to_string(),
            value: filler(),
        })
    }

    #[test]
    fn detects_read_method_shadow_in_init() {
        let mut m = Module::new("t");
        m.init = vec![put_value_set("readUInt8")];
        assert!(module_shadows_buffer_read_method(&m));
    }

    #[test]
    fn detects_dot_shadow_nested_in_control_flow() {
        let mut m = Module::new("t");
        m.init = vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![property_set("readInt32BE")],
            else_branch: None,
        }];
        assert!(module_shadows_buffer_read_method(&m));
    }

    #[test]
    fn ignores_non_read_method_names() {
        let mut m = Module::new("t");
        // `writeUInt8` is a WRITE method (no read-intrinsic fold to protect),
        // and `foo` is an ordinary expando — neither can shadow a folded read.
        m.init = vec![put_value_set("writeUInt8"), property_set("foo")];
        assert!(!module_shadows_buffer_read_method(&m));
    }

    #[test]
    fn empty_module_does_not_shadow() {
        assert!(!module_shadows_buffer_read_method(&Module::new("t")));
    }

    #[test]
    fn detects_shadow_in_class_field_initializer() {
        // `class C { tag = ((buf as any).readDoubleLE = fn); }` — the shadow
        // lives in `fields[*].init`, not the constructor body (#6405 review).
        let mut m = Module::new("t");
        m.classes.push(class_with_fields(vec![field_with_init(
            "tag",
            put_value_set_expr("readDoubleLE"),
        )]));
        assert!(module_shadows_buffer_read_method(&m));
    }

    #[test]
    fn plain_class_field_does_not_shadow() {
        // A field NAMED like a read method (`class C { readUInt8 = 0; }`) is a
        // user-class member, not an own-prop assignment on a Buffer — it must
        // NOT trip the scan.
        let mut m = Module::new("t");
        m.classes.push(class_with_fields(vec![field_with_init(
            "readUInt8",
            Expr::Integer(0),
        )]));
        assert!(!module_shadows_buffer_read_method(&m));
    }
}
