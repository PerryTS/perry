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

/// `line` with its `thread_local` specifier (the default general-dynamic
/// model, or `initialexec`) replaced by `thread_local(localexec)`, or `None`
/// when it has none (not a thread-local, or already local-exec).
///
/// The specifier is one of the keywords before `global`/`constant`; only those
/// are read. The initializer can spell the same word: a string image's bytes
/// (highlight.js's C++ keyword list in Claude Code) once grew by the 11 bytes
/// of `(localexec)` and no longer matched its declared array length.
pub(crate) fn with_local_exec_tls(line: &str) -> Option<String> {
    let (lhs, rhs) = line.split_once(" = ")?;
    let mut at = 0;
    for word in rhs.split(' ') {
        match word {
            "global" | "constant" => return None,
            "thread_local" | "thread_local(initialexec)" => {
                return Some(format!(
                    "{lhs} = {}thread_local(localexec) {}",
                    &rhs[..at],
                    rhs.get(at + word.len() + 1..).unwrap_or("")
                ));
            }
            _ => at += word.len() + 1,
        }
    }
    None
}

/// True when a global definition already carries LOCAL linkage
/// (`private`/`internal`). Those are the only definitions whose promotion
/// `codegen_unit_parts` may skip: a local symbol cannot collide with anything
/// — not another unit of this module, not another module's copy of a
/// same-named global — so leaving it alone is inert at link time. Dropping the
/// Split an optional `thread_local` / `thread_local(<model>)` specifier off a
/// global's post-linkage text (#10399). Returns `(tls, rest)`, where `tls` is
/// `""` when there is none.
///
/// LLVM's grammar puts the TLS specifier AFTER linkage and BEFORE
/// `global`/`constant`, so anything that takes a definition apart to rebuild
/// it has to carry this across — a declaration that loses `thread_local`
/// names a different (non-TLS) symbol than the definition.
pub(crate) fn split_thread_local(s: &str) -> (&str, &str) {
    const KW: &str = "thread_local";
    let t = s.trim_start();
    if !t.starts_with(KW) {
        return ("", t);
    }
    let after = &t[KW.len()..];
    let end = if after.starts_with('(') {
        match after.find(')') {
            Some(i) => KW.len() + i + 1,
            None => return ("", t),
        }
    } else if after.starts_with(char::is_whitespace) {
        KW.len()
    } else {
        // A symbol whose text merely starts with the keyword.
        return ("", t);
    };
    (&t[..end], t[end..].trim_start())
}

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

/// Bind a generated definition to this image. Imports and LLVM's appending
/// lists retain their contracts; local linkage already implies dso_local.
/// Visibility is part of the IR, so both unit emitters see the same fact.
pub(crate) fn hidden_global_definition(line: &str) -> String {
    let Some((name, rhs)) = line.split_once(" = ") else {
        return line.to_string();
    };
    if has_local_linkage(line)
        || rhs.starts_with("external ")
        || rhs.starts_with("appending ")
        || name.starts_with("@llvm.")
    {
        return line.to_string();
    }
    let rest = strip_leading_linkage(rhs);
    let linkage = &rhs[..rhs.len() - rest.len()];
    let (_, rest) = take_keyword(rest, &["dso_local", "dso_preemptable"]);
    let (_, rest) = take_keyword(rest, &["default", "hidden", "protected"]);
    format!("{name} = {linkage}hidden {rest}")
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
        Some((lhs, rhs)) => hidden_global_definition(&format!(
            "{} = linkonce_odr {}",
            lhs,
            strip_leading_linkage(rhs.trim_start())
        )),
        None => line.to_string(),
    }
}

/// [`promote_global_for_units`] for an EXTERNAL definition: `weak_odr`.
///
/// Like `linkonce_odr` it lets the linker fold same-named copies — from
/// several codegen units, or from another module — into one. Unlike it, it is
/// not discardable: LLVM must emit the definition even when nothing in this
/// unit uses it. An external definition can be named only from another
/// object (a ConstFn body's `$info` is referenced by nothing but the static
/// shape-seed object), and a discardable sole copy is removed from its owner
/// before that object is ever linked against it.
pub(crate) fn promote_external_global_for_units(line: &str) -> String {
    if line.contains(" = external ") {
        return line.to_string();
    }
    match line.split_once(" = ") {
        Some((lhs, rhs)) => format!(
            "{} = weak_odr {}",
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
/// The result is a plain STRONG definition; promoted locals are hidden. The
/// symbol's NAME must be unique across the whole program, not just the
/// module: `.str.N` constants only satisfy that through
/// [`LlModule::set_symbol_prefix`].
pub(crate) fn make_unique_owner_global(line: &str) -> String {
    if line.contains(" = external ") {
        return line.to_string();
    }
    // An explicit weak data definition is intended to coalesce across
    // modules, not merely across this module's units. Keep that ownership
    // contract; weak_odr is non-discardable even when the owner's references
    // optimize away. Image strings carry no function/associative targets.
    if line.split_once(" = ").is_some_and(|(_, rhs)| {
        let Some(rhs) = rhs.strip_prefix("weak_odr ") else {
            return false;
        };
        let (_, rhs) = take_keyword(rhs, &["dso_local", "dso_preemptable"]);
        let (_, rhs) = take_keyword(rhs, &["default", "hidden", "protected"]);
        let (_, rhs) = take_keyword(rhs, &["unnamed_addr", "local_unnamed_addr"]);
        rhs.starts_with("constant ")
    }) {
        return line.to_string();
    }
    match line.split_once(" = ") {
        Some((lhs, rhs)) => {
            let definition = format!("{} = {}", lhs, strip_leading_linkage(rhs.trim_start()));
            if has_local_linkage(line) {
                hidden_global_definition(&definition)
            } else {
                definition
            }
        }
        None => line.to_string(),
    }
}

/// The aliasee symbol (`@name`) of an alias definition
/// (`@a = [linkage] ... alias TY, <constant naming @aliasee>`), or `None`
/// when `line` is not an alias.
pub(crate) fn alias_target(line: &str) -> Option<&str> {
    let (_, rhs) = line.split_once(" = ")?;
    let rest = &rhs[rhs.find("alias ")?..];
    if !rhs[..rhs.len() - rest.len()]
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '(' | ')'))
    {
        return None;
    }
    let at = rest.find('@')?;
    let tail = &rest[at..];
    let end = tail[1..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')))
        .map_or(tail.len(), |i| i + 1);
    Some(&tail[..end])
}

pub(crate) fn external_decl_for_global(line: &str) -> Option<String> {
    if line.contains(" = external ") {
        return Some(line.to_string());
    }
    let (name, rhs) = line.split_once(" = ")?;
    let rhs = strip_leading_linkage(rhs.trim_start());
    // After the linkage, LLVM orders a global's prefix as
    //   [preemption] [visibility] [thread_local] [unnamed_addr] global|constant
    // A declaration in another unit must keep the preemption and visibility of
    // the definition it names. Closure-info records are emitted
    // `hidden constant` (fn_info.rs), and on ELF/COFF, where globals are not
    // replicated, every cross-unit reference to one is declared here.
    let (preemption, rhs) = take_keyword(rhs, &["dso_local", "dso_preemptable"]);
    let (visibility, rhs) = take_keyword(rhs, &["default", "hidden", "protected"]);
    // Promoting a local definition must not introduce symbol preemption.
    let visibility = if has_local_linkage(line) {
        "hidden"
    } else {
        visibility
    };
    // #10399: keep the TLS specifier — `@g = external global i8` and
    // `@g = external thread_local global i8` are different symbols to LLVM.
    let (tls, rhs) = split_thread_local(rhs);
    // `unnamed_addr` describes the definition only; the declaration drops it.
    let (_, rhs) = take_keyword(rhs, &["unnamed_addr", "local_unnamed_addr"]);
    let (initialized, rhs) = take_keyword(rhs, &["externally_initialized"]);
    // An alias is declared, in a unit that does not define it, as an
    // external global of its value type: object files see only a symbol.
    if let Some(rest) = rhs.strip_prefix("alias ") {
        let ty = rest.split_once(',')?.0.trim();
        let prefix: String = [preemption, visibility, tls]
            .iter()
            .filter(|keyword| !keyword.is_empty())
            .map(|keyword| format!("{keyword} "))
            .collect();
        return Some(format!("{name} = external {prefix}global {ty}"));
    }
    let (kind, rest) = if let Some(rest) = rhs.strip_prefix("constant ") {
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
    let prefix: String = [preemption, visibility, tls, initialized]
        .iter()
        .filter(|keyword| !keyword.is_empty())
        .map(|keyword| format!("{keyword} "))
        .collect();
    Some(format!(
        "{name} = external {prefix}{kind} {}",
        &rest[..ty_end]
    ))
}

/// Take one of `keywords` from the front of `s` when it is followed by
/// whitespace, returning `(keyword, rest)`, or `("", s)` when none is there.
fn take_keyword<'a>(s: &'a str, keywords: &[&'static str]) -> (&'static str, &'a str) {
    let t = s.trim_start();
    for keyword in keywords {
        if let Some(rest) = t.strip_prefix(keyword) {
            if rest.starts_with(char::is_whitespace) {
                return (keyword, rest.trim_start());
            }
        }
    }
    ("", t)
}

/// `cold` placement hint only: says nothing about unwinding.
const COLD_HELPERS: &[&str] = &[
    // These calls sit behind cache-hit / inactive-marking guards. Keep
    // register saves and code layout focused on the inline continuation.
    // `cold` is only a profitability hint: both calls remain fully
    // memory-clobbering and the cache miss remains GC-capable/throwing.
    "js_object_get_field_slow",
    "js_object_get_field_nonptr",
    "js_object_get_field_value",
    "js_global_get_field_slow",
    "js_write_barrier_root_nanbox",
    // S2 (`expr/ic_fast_split.rs`): the collecting continuations behind
    // a declined GC-leaf hit. Still throwing, still GC-capable.
    // The prologue stack check's overflow arm (`expr/stack_guard.rs`).
    // `-> !` in the runtime, but declared without `noreturn` here: the
    // clone-call check rejoins its call after it.
    "js_stack_overflow",
];

// The two memory-contract lists below are audited facts about what a
// helper's body reads and writes, which no table carries. Whether a call can
// unwind is not part of them: `nounwind` comes only from the table of the
// archive being linked (see `helper_decl_attrs`).

/// `willreturn readnone`.
const PURE_HELPERS: &[&str] = &[
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
    "js_nanbox_pointer",
    "js_nanbox_get_pointer",
    "js_typed_f64_arg_guard",
    "js_typed_i32_arg_guard",
    "js_typed_i1_arg_guard",
    "js_typed_i1_arg_to_raw",
    "js_typed_i32_arg_to_raw",
    "js_typed_string_arg_guard",
];

/// `willreturn readonly`.
const READONLY_HELPERS: &[&str] = &[
    // READONLY — verified: tag ladder plus reads of StringHeader.utf16_len
    // (via is_valid_string_ptr, a pure magnitude check) and BigInt limbs
    // (js_bigint_is_zero via clean_bigint_ptr, pure bit cleanup). No
    // registry/lock access, no allocation, no throw, no writes.
    "js_is_truthy",
    // string/compare.rs: pointer magnitude guards, immutable byte views,
    // bounded word scans / UTF-16 decoder iteration only. No allocation,
    // GC, locks, writes, or JavaScript coercion. Read-any (not argmem:
    // operands are i64 handles) orders it against every GC-capable call.
    // js_string_compare_value is NOT eligible: number coercion allocates.
    "js_string_compare",
];

/// Function attributes for a runtime-helper `declare` line (#6082 tranche 1).
///
/// Without attributes, -O3 must treat every `js_*` call as "may read and
/// write all memory, may not return, may unwind": no CSE/LICM/DCE across any
/// helper call. Two sources re-enable those optimizations, and each states
/// only what it knows:
///
/// * **Unwinding: the table.** `nounwind` is emitted exactly when the GC
///   effects table of the archive being linked proves the symbol
///   `throws=no` ([`GcEffects::runtime_throws`], the same fact that picks
///   `call` over `invoke` in `eh_mode::nothrow_callee`). No hand list states
///   it, so a declaration can never promise more than the linked bytes do.
///   Under the all-unsafe table (no archive, Geisterhand, UI-lib runtimes)
///   nothing is `nounwind`.
/// * **Memory: the audited contracts.** No table carries what a body reads
///   or writes, so the two lists above stay hand-audited:
///   - PURE = `willreturn readnone`. Invariant: the helper's Rust body
///     (transitively) performs NaN-box BIT manipulation only: no loads, no
///     stores, no allocation, no GC trigger, and it is total over arbitrary
///     input bits (no panic, no UB), so LLVM may CSE/hoist/sink it freely.
///   - READONLY = `willreturn readonly`. Invariant: the helper may READ heap
///     memory (string headers, BigInt limbs) but never writes, never
///     allocates, never triggers GC, never takes a lock. LLVM may CSE/LICM it
///     across write-free regions, but must still order it against any
///     possibly-writing call, which keeps it correct w.r.t. the moving GC,
///     because every GC-capable helper stays maximally clobbering.
///   A call LLVM may delete when unused must also be `nounwind`: `willreturn`
///   alone keeps a may-unwind call (LLVM counts unwinding as a side effect).
///
/// SYNTAX NOTE: spelled with the LEGACY `readnone` / `readonly` function
/// attributes, NOT the modern `memory(none)` / `memory(read)`: old LLVM asm
/// parsers (e.g. the Apple clang 15 shipped on macos-14 CI runners, and any
/// user clang predating LLVM's `memory` attribute) reject the modern
/// spelling, which killed every `--backend llvm` compile through that clang
/// (caught by the simctl iOS smoke gating the v0.5.1265 release). New parsers
/// still accept the legacy spelling and auto-upgrade it to the equivalent
/// `memory(...)` form, so semantics are identical everywhere.
///
/// SOUNDNESS NOTES (read before adding a memory-contract entry):
/// * Deliberately reads-any (`readonly`, i.e. `memory(read)`), NOT an
///   argmem-scoped form: helper args are f64 NaN-boxes, not LLVM pointer
///   arguments, so `argmem` would mean "reads no memory at all" and
///   license CSE/DSE across real heap reads.
/// * Anything that can allocate or trigger GC gets NO memory contract: the
///   moving GC's reload discipline depends on those calls staying maximally
///   clobbering.
///
/// Audited and rejected (do not re-add without a new audit):
/// `js_nanbox_string` (allocates an empty string for null input),
/// `js_get_string_pointer_unified` / `js_typed_string_arg_to_raw`
/// (materialize SSO strings onto the heap = allocation),
/// `js_typed_f64_arg_to_raw` (routes through `js_number_coerce`, whose
/// string/object paths read+parse and reach ToPrimitive),
/// `js_value_length_f64` (Buffer/TypedArray registry lookups take locks —
/// a lock acquisition writes memory).
fn helper_decl_contract(name: &str) -> HelperDeclContract {
    if COLD_HELPERS.contains(&name) {
        HelperDeclContract::Cold
    } else if PURE_HELPERS.contains(&name) {
        HelperDeclContract::Pure
    } else if READONLY_HELPERS.contains(&name) {
        HelperDeclContract::ReadOnly
    } else {
        HelperDeclContract::Conservative
    }
}

enum HelperDeclContract {
    Conservative,
    Cold,
    Pure,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperMemoryEffect {
    None,
    ReadOnly,
    MayWrite,
}

pub(crate) fn helper_memory_effect(name: &str) -> HelperMemoryEffect {
    match helper_decl_contract(name) {
        HelperDeclContract::Pure => HelperMemoryEffect::None,
        HelperDeclContract::ReadOnly => HelperMemoryEffect::ReadOnly,
        _ => HelperMemoryEffect::MayWrite,
    }
}

/// The trailing function attributes of `name`'s `declare` line under
/// `effects` (see [`helper_decl_contract`]): empty, or a leading space
/// followed by the attributes.
pub(crate) fn helper_decl_attrs(effects: &crate::gc_call_effects::GcEffects, name: &str) -> String {
    let mut attrs = String::new();
    if !effects.runtime_throws(name) {
        attrs.push_str(" nounwind");
    }
    attrs.push_str(match helper_decl_contract(name) {
        HelperDeclContract::Conservative => "",
        HelperDeclContract::Cold => " cold",
        HelperDeclContract::Pure => " willreturn readnone",
        HelperDeclContract::ReadOnly => " willreturn readonly",
    });
    attrs
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
    } else if let Some(cc) = crate::expr::inline_birth::calling_convention(&f.name) {
        format!("{cc} ")
    } else {
        String::new()
    };
    format!("declare {}{} @{}({})", cconv, f.return_type, f.name, params)
}

/// Render a function with external linkage forced, promoting an `internal` /
/// `private` definition so cross-unit calls can bind to it. Names are
/// module-prefixed and unique, so promotion never collides.
pub(crate) fn render_fn_external_with_gc_leaf_callees(
    f: &LlFunction,
    gc_leaf_callees: &HashSet<String>,
) -> String {
    force_external_linkage(f, f.to_ir_with_gc_leaf_callees(gc_leaf_callees))
}

/// `ir`, `f`'s rendering, with an `internal` / `private` definition promoted
/// to external linkage (see [`render_fn_external_with_gc_leaf_callees`]).
pub(crate) fn force_external_linkage(f: &LlFunction, ir: String) -> String {
    // Native split-unit headers already force every definition external.
    // Diagnostics use text to attach call-site metadata; preserve that same
    // linkage so a helper needed only by another unit cannot be discarded.
    if f.linkage == "internal"
        || f.linkage == "private"
        || (crate::fallback_diag::enabled() && !f.linkage.is_empty())
    {
        return ir.replacen(
            &format!("define {} ", f.linkage),
            &format!("define {}", LlFunction::split_unit_linkage(&f.linkage)),
            1,
        );
    }
    ir
}
