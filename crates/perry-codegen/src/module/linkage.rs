//! LLVM linkage / symbol-reference helpers for `LlModule`, split from
//! `module.rs` for the 2000-line file cap. #9610 grew this block when
//! single-unit globals gained their own linkage so zero-init caches stay
//! in `__bss`.

use std::collections::HashSet;

use crate::function::LlFunction;

/// Strip a leading LLVM linkage keyword from a global's post-`=` text, if
/// present. Linkage comes before `unnamed_addr`/`constant`/`global` in the
/// grammar, so this leaves the rest of the definition intact.
pub(crate) fn strip_leading_linkage(s: &str) -> &str {
    for kw in [
        "private ",
        "internal ",
        "linkonce_odr ",
        "linkonce ",
        "weak_odr ",
        "weak ",
        "common ",
        "available_externally ",
    ] {
        if let Some(rest) = s.strip_prefix(kw) {
            return rest;
        }
    }
    s
}

/// Symbol name of a global/string definition line (`@name = ...`).
pub(crate) fn global_symbol_name(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if !line.starts_with('@') {
        return None;
    }
    let end = line.find(" = ")?;
    Some(&line[..end])
}

/// Collect every `@symbol` referenced in a chunk of IR text.
pub(crate) fn collect_symbol_refs(text: &str, out: &mut HashSet<String>) {
    let b = text.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'@' {
            let start = i;
            i += 1;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b'$' | b'-'))
            {
                i += 1;
            }
            if i > start + 1 {
                out.insert(text[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
}

pub(crate) fn metadata_definition_id(line: &str) -> Option<u32> {
    let rest = line.trim_start().strip_prefix('!')?;
    let (digits, _) = rest.split_once(" =")?;
    digits.parse().ok()
}

/// Collect numeric LLVM metadata references (`!123`) from instructions or
/// metadata definitions. Named metadata does not occur in Perry's alias tail.
pub(crate) fn collect_metadata_refs(text: &str, out: &mut HashSet<u32>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'!' || i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i + 1;
        i = start;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if let Ok(id) = text[start..i].parse() {
            out.insert(id);
        }
    }
}

/// True when a global definition already carries LOCAL linkage
/// (`private`/`internal`). Those are the only definitions whose promotion
/// `codegen_unit_parts` may skip: a local symbol cannot collide with anything
/// — not another unit of this module, not another module's copy of a
/// same-named global — so leaving it alone is inert at link time. Dropping the
/// promotion on a strong external definition would NOT be: `linkonce_odr` is
/// what lets ld64 coalesce two modules' same-named globals instead of
/// reporting a duplicate symbol.
pub(crate) fn has_local_linkage(line: &str) -> bool {
    match line.split_once(" = ") {
        Some((_, rhs)) => {
            let rhs = rhs.trim_start();
            rhs.starts_with("private ") || rhs.starts_with("internal ")
        }
        None => false,
    }
}

/// Rewrite a module-global definition so it is safe to duplicate across
/// codegen units (#5391). Local-linkage (`private`/`internal`) and bare
/// external definitions are promoted to `linkonce_odr`, so the linker keeps a
/// single copy when the same global is emitted into multiple units. `external`
/// declarations (no initializer) are returned unchanged — duplicating a
/// declaration is harmless.
///
/// Apply this ONLY to a global that really is emitted more than once (#9610).
/// `linkonce_odr` is weak-for-linker, and `TargetLoweringObjectFileMachO`
/// routes every weak-for-linker global to the coalesced data section before it
/// ever consults `SectionKind::isBSS()` — so promoting a `zeroinitializer`
/// global that only one unit defines moves it out of zerofill `__DATA,__bss`
/// and writes its zeros into the file.
pub(crate) fn promote_global_for_units(line: &str) -> String {
    if line.contains(" = external ") {
        return line.to_string();
    }
    match line.split_once(" = ") {
        Some((lhs, rhs)) => format!(
            "{} = linkonce_odr {}",
            lhs,
            strip_leading_linkage(rhs.trim_start())
        ),
        None => line.to_string(),
    }
}

/// Give a generated global one non-discardable definition. On every
/// non-Mach-O target (ELF and COFF — see `replicate_globals`) each global has
/// a unique owning codegen unit; leaving that sole definition as
/// `linkonce_odr` lets LLVM discard it when all references in the owner happen
/// to optimize away, even though other object files still reference it.
///
/// The result is a plain STRONG definition with default visibility, so the
/// symbol's NAME must be unique across the whole program, not just the
/// module: `.str.N` constants only satisfy that through
/// [`LlModule::set_symbol_prefix`].
pub(crate) fn make_unique_owner_global(line: &str) -> String {
    if line.contains(" = external ") {
        return line.to_string();
    }
    match line.split_once(" = ") {
        Some((lhs, rhs)) => format!("{} = {}", lhs, strip_leading_linkage(rhs.trim_start())),
        None => line.to_string(),
    }
}

pub(crate) fn external_decl_for_global(line: &str) -> Option<String> {
    if line.contains(" = external ") {
        return Some(line.to_string());
    }
    let (name, rhs) = line.split_once(" = ")?;
    let rhs = strip_leading_linkage(rhs.trim_start());
    let (kind, rest) = if let Some(rest) = rhs.strip_prefix("unnamed_addr constant ") {
        ("constant", rest)
    } else if let Some(rest) = rhs.strip_prefix("constant ") {
        ("constant", rest)
    } else if let Some(rest) = rhs.strip_prefix("global ") {
        ("global", rest)
    } else {
        return None;
    };
    let rest = rest.trim_start();
    let ty_end = match rest.as_bytes().first().copied() {
        Some(b'[') | Some(b'{') | Some(b'<') => {
            let (mut square, mut curly, mut angle) = (0i32, 0i32, 0i32);
            let mut end = None;
            for (i, b) in rest.bytes().enumerate() {
                match b {
                    b'[' => square += 1,
                    b']' => square -= 1,
                    b'{' => curly += 1,
                    b'}' => curly -= 1,
                    b'<' => angle += 1,
                    b'>' => angle -= 1,
                    _ => {}
                }
                if square == 0 && curly == 0 && angle == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            end?
        }
        _ => rest.find(char::is_whitespace).unwrap_or(rest.len()),
    };
    Some(format!("{name} = external {kind} {}", &rest[..ty_end]))
}

/// Attribute-group suffix for a runtime-helper `declare` line, keyed by
/// helper name (#6082 tranche 1).
///
/// Without attributes, -O3 must treat every `js_*` call as "may read and
/// write all memory, may not return" — no CSE/LICM/DCE across any helper
/// call. The two groups below re-enable those optimizations for a small,
/// individually audited allowlist:
///
/// * `#2` (PURE) = `nounwind willreturn readnone`. Invariant: the
///   helper's Rust body (transitively) performs NaN-box BIT manipulation
///   only — no loads, no stores, no allocation, no GC trigger, no
///   `js_throw`/longjmp, and it is total over arbitrary input bits (no
///   panic, no UB), so LLVM may CSE/hoist/sink/delete it freely.
/// * `#3` (READONLY) = `nounwind willreturn readonly`. Invariant: the
///   helper may READ heap memory (string headers, BigInt limbs) but never
///   writes, never allocates, never triggers GC, never takes a lock, and
///   never throws. LLVM may CSE/LICM it across write-free regions and
///   delete unused calls, but must still order it against any
///   possibly-writing call — which keeps it correct w.r.t. the moving GC,
///   because every GC-capable helper stays maximally clobbering.
///
/// SYNTAX NOTE: the groups are spelled with the LEGACY `readnone` /
/// `readonly` function attributes, NOT the modern `memory(none)` /
/// `memory(read)` — old LLVM asm parsers (e.g. the Apple clang 15 shipped
/// on macos-14 CI runners, and any user clang predating LLVM's `memory`
/// attribute) reject the modern spelling with "unterminated attribute
/// group", which killed every `--backend llvm` compile through that clang
/// (caught by the simctl iOS smoke gating the v0.5.1265 release). New
/// parsers still accept the legacy spelling and auto-upgrade it to the
/// equivalent `memory(...)` form, so semantics are identical everywhere.
///
/// SOUNDNESS NOTES (read before adding an entry):
/// * Deliberately reads-any (`readonly`, i.e. `memory(read)`), NOT an
///   argmem-scoped form: helper args are f64 NaN-boxes, not LLVM pointer
///   arguments, so `argmem` would mean "reads no memory at all" and
///   license CSE/DSE across real heap reads.
/// * Anything that can allocate or trigger GC gets NO group — the moving
///   GC's shadow-stack reload discipline depends on those calls staying
///   maximally clobbering.
/// * Anything that can reach `js_throw` (raises through the unwinder) gets NO group —
///   `willreturn` would let DCE delete a throwing call whose result is
///   unused, silently dropping the exception.
///
/// Audited and rejected (do not re-add without a new audit):
/// `js_nanbox_string` (allocates an empty string for null input),
/// `js_get_string_pointer_unified` / `js_typed_string_arg_to_raw`
/// (materialize SSO strings onto the heap = allocation),
/// `js_typed_f64_arg_to_raw` (routes through `js_number_coerce`, whose
/// string/object paths read+parse and reach ToPrimitive),
/// `js_value_length_f64` (Buffer/TypedArray registry lookups take locks —
/// a lock acquisition writes memory).
pub(crate) fn helper_decl_attrs(name: &str) -> &'static str {
    match name {
        // PURE — each verified: pure bit tests/masking on the f64/i64 args,
        // total over arbitrary bits, no memory access anywhere in the body.
        //   js_nanbox_pointer        value/nanbox.rs — tag ladder, 0 → TAG_NULL
        //   js_nanbox_get_pointer    value/nanbox.rs — mask ladder, no deref
        //   js_typed_f64_arg_guard   native_abi.rs — tag-band check
        //   js_typed_i32_arg_guard   native_abi.rs — tag check + finite/fract/range
        //   js_typed_i1_arg_guard    native_abi.rs — bits == TAG_TRUE|TAG_FALSE
        //   js_typed_i1_arg_to_raw   native_abi.rs — bits == TAG_TRUE
        //   js_typed_i32_arg_to_raw  native_abi.rs — bit extract / saturating cast
        //   js_typed_string_arg_guard native_abi.rs — STRING/SHORT_STRING tag check
        "js_nanbox_pointer"
        | "js_nanbox_get_pointer"
        | "js_typed_f64_arg_guard"
        | "js_typed_i32_arg_guard"
        | "js_typed_i1_arg_guard"
        | "js_typed_i1_arg_to_raw"
        | "js_typed_i32_arg_to_raw"
        | "js_typed_string_arg_guard" => " #2",
        // READONLY — verified: tag ladder plus reads of StringHeader.utf16_len
        // (via is_valid_string_ptr, a pure magnitude check) and BigInt limbs
        // (js_bigint_is_zero via clean_bigint_ptr, pure bit cleanup). No
        // registry/lock access, no allocation, no throw, no writes.
        "js_is_truthy" => " #3",
        // NOUNWIND+WILLRETURN only (#4, repsel Phase 4a.0) — each verified
        // (`typed_feedback.rs` / `array/header.rs`): no `js_throw` (longjmp)
        // anywhere in the body, every loop bounded by the 16M length/capacity
        // sanity caps, no allocation, no GC trigger. They are NOT readonly:
        // the numeric guards' first-touch path REBUILDS unmarked arrays into
        // raw-f64 layout (slot writes + flag store), feedback mode
        // (`PERRY_TYPED_FEEDBACK`, a runtime env check) records observations,
        // and `js_array_numeric_value_to_raw_f64`'s ClassRef probe takes
        // registry RwLock reads (a lock word write). #6082 trap notes apply:
        // argmem is unsound for NaN-box args, and `willreturn` is only
        // admissible because these helpers cannot reach `js_throw` — any
        // divergence is a Rust panic-abort, which never resumes the program.
        "js_typed_feedback_plain_array_index_get_guard"
        | "js_typed_feedback_numeric_array_index_get_guard"
        | "js_typed_feedback_plain_array_index_set_guard"
        | "js_typed_feedback_numeric_array_index_set_guard"
        | "js_typed_feedback_numeric_array_push_guard"
        | "js_array_numeric_value_to_raw_f64" => " #4",
        _ => "",
    }
}

/// Synthesize an external `declare` line matching a locally-defined function's
/// signature, so a codegen unit that calls it (but does not define it) resolves
/// the call at link time.
pub(crate) fn declare_line_for(f: &LlFunction) -> String {
    let params = f
        .params
        .iter()
        .map(|(t, _)| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    // #8175: a codegen unit that calls a promoted `preserve_nonecc` clone
    // binds through this declare; the convention must ride along or the
    // cross-unit ABI silently splits from the defining unit's.
    let cconv: String = if f.is_preserve_none() {
        format!("{} ", crate::inst::PRESERVE_NONE_CC)
    } else {
        String::new()
    };
    format!("declare {}{} @{}({})", cconv, f.return_type, f.name, params)
}

/// Render a function with external linkage forced, promoting an `internal` /
/// `private` definition so cross-unit calls can bind to it. Names are
/// module-prefixed and unique, so promotion never collides.
pub(crate) fn render_fn_external(f: &LlFunction) -> String {
    render_fn_external_with_gc_leaf_callees(f, &HashSet::new())
}

pub(crate) fn render_fn_external_with_gc_leaf_callees(
    f: &LlFunction,
    gc_leaf_callees: &HashSet<String>,
) -> String {
    let ir = f.to_ir_with_gc_leaf_callees(gc_leaf_callees);
    if f.linkage == "internal" || f.linkage == "private" {
        return ir.replacen(&format!("define {} ", f.linkage), "define ", 1);
    }
    ir
}
