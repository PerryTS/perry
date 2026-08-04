//! LLVM IR module builder — the top-level `.ll` file.
//!
//! Port of `anvil/src/llvm/module.ts`. Tracks:
//! - external function declarations (deduped; skipped in output if the same
//!   name is also defined in the module, to avoid declare+define conflicts)
//! - string constants (pooled, UTF-8 encoded with a null terminator)
//! - global variables (external, internal, initialized)
//! - function definitions
//!
//! `to_ir()` assembles the pieces into a complete `.ll` file with the target
//! triple header.

use std::collections::{BTreeMap, HashSet};

use crate::block::FpFlags;
use crate::function::LlFunction;
use crate::native_value::NativeRepRecord;
use crate::types::LlvmType;

/// Strip a leading LLVM linkage keyword from a global's post-`=` text, if
/// present. Linkage comes before `unnamed_addr`/`constant`/`global` in the
/// grammar, so this leaves the rest of the definition intact.
fn strip_leading_linkage(s: &str) -> &str {
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

/// Rewrite a module-global definition so it is safe to duplicate across
/// codegen units (#5391). Local-linkage (`private`/`internal`) and bare
/// external definitions are promoted to `linkonce_odr`, so the linker keeps a
/// single copy when the same global is emitted into multiple units. `external`
/// declarations (no initializer) are returned unchanged — duplicating a
/// declaration is harmless.

/// Symbol name of a global/string definition line (`@name = ...`).
fn global_symbol_name(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if !line.starts_with('@') {
        return None;
    }
    let end = line.find(" = ")?;
    Some(&line[..end])
}

/// Collect every `@symbol` referenced in a chunk of IR text.
fn collect_symbol_refs(text: &str, out: &mut HashSet<String>) {
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

fn promote_global_for_units(line: &str) -> String {
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
    format!("declare {} @{}({})", f.return_type, f.name, params)
}

/// Render a function with external linkage forced, promoting an `internal` /
/// `private` definition so cross-unit calls can bind to it. Names are
/// module-prefixed and unique, so promotion never collides.
fn render_fn_external(f: &LlFunction) -> String {
    let ir = f.to_ir();
    if f.linkage == "internal" || f.linkage == "private" {
        return ir.replacen(&format!("define {} ", f.linkage), "define ", 1);
    }
    ir
}

fn push_statepoint_declarations(ir: &mut String) {
    ir.push_str(
        "declare token @llvm.experimental.gc.statepoint.p0(i64 immarg, i32 immarg, ptr, \
         i32 immarg, i32 immarg, ...)\n\
         declare ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token, i32 immarg, \
         i32 immarg)\n",
    );
    for (suffix, ty) in [
        ("i1", "i1"),
        ("i8", "i8"),
        ("i16", "i16"),
        ("i32", "i32"),
        ("i64", "i64"),
        ("i128", "i128"),
        ("f32", "float"),
        ("f64", "double"),
        ("p0", "ptr"),
    ] {
        ir.push_str(&format!(
            "declare {ty} @llvm.experimental.gc.result.{suffix}(token)\n"
        ));
    }
}

pub struct LlModule {
    pub target_triple: String,
    declarations: Vec<(String, String)>, // (name, full "declare …" line)
    declared_names: HashSet<String>,
    functions: Vec<LlFunction>,
    globals: Vec<String>,
    string_constants: Vec<String>,
    string_counter: u32,
    /// Extra numbered metadata nodes emitted after `!0 = !{}`. Used by
    /// the buffer alias-scope system to declare per-buffer scopes and
    /// noalias sets so LLVM's LoopVectorizer can prove different buffers
    /// don't alias.
    metadata_lines: Vec<String>,
    /// Module-wide counter for inline cache globals (`perry_ic_N`).
    /// Must be unique across all functions in the module.
    pub ic_counter: u32,
    /// Module-wide counter for buffer alias-scope ids. Each function's
    /// `FnCtx` reads this as its `buffer_alias_base` at creation, then
    /// after the function lowers its body the counter is bumped by the
    /// number of scopes that function allocated. Must be unique across
    /// every function in the module so `!alias.scope !201` references
    /// emitted on loads/stores match the metadata nodes emitted once
    /// at the end of `compile_module` (closes #71).
    pub buffer_alias_counter: u32,
    pub(crate) native_rep_records: Vec<NativeRepRecord>,
    fp_flags: FpFlags,
}

impl LlModule {
    pub fn new(target_triple: impl Into<String>) -> Self {
        Self::new_with_fp_flags(target_triple, FpFlags::default())
    }

    pub fn new_with_fp_flags(target_triple: impl Into<String>, fp_flags: FpFlags) -> Self {
        Self {
            target_triple: target_triple.into(),
            declarations: Vec::new(),
            declared_names: HashSet::new(),
            functions: Vec::new(),
            globals: Vec::new(),
            string_constants: Vec::new(),
            string_counter: 0,
            metadata_lines: Vec::new(),
            ic_counter: 0,
            buffer_alias_counter: 0,
            native_rep_records: Vec::new(),
            fp_flags,
        }
    }

    /// Append a raw metadata definition line (e.g. `!1 = distinct !{!1}`).
    /// Emitted after `!0 = !{}` in the module IR.
    pub fn add_metadata_line(&mut self, line: String) {
        self.metadata_lines.push(line);
    }

    /// Declare an external function (FFI import). Deduped by name — later
    /// calls with the same name are no-ops. If a function with the same name
    /// is later *defined* in this module, the declaration is dropped at
    /// `to_ir` time so LLVM doesn't see both.
    pub fn declare_function(
        &mut self,
        name: &str,
        return_type: LlvmType,
        param_types: &[LlvmType],
    ) {
        if self.declared_names.contains(name) {
            return;
        }
        self.declared_names.insert(name.to_string());
        let param_str = param_types.join(", ");
        // Verified-pure runtime helpers get the #2/#3 optimization groups
        // (#6082) — see `helper_decl_attrs` for the audit invariants. The
        // lookup is name-keyed here in the single declaration funnel so
        // every declaration path agrees on the attributes.
        let attrs = helper_decl_attrs(name);
        self.declarations.push((
            name.to_string(),
            format!("declare {} @{}({}){}", return_type, name, param_str, attrs),
        ));
    }

    /// SEH funclets (#7302): true when this module targets windows-msvc AND
    /// contains try/catch, i.e. when its EH lowering is
    /// `catchswitch`/`catchpad`/`catchret` rather than Itanium landing pads.
    ///
    /// The in-process LLVM reader can build `invoke`/`landingpad` but NOT
    /// the funclet forms: inkwell 0.9 exposes no `build_catch_switch` /
    /// `build_catch_pad` / `build_catch_ret` (only an opcode enum for
    /// reading them), so constructing them needs raw `llvm-sys` FFI. Until
    /// that lands, such modules take the textual path — declining costs
    /// nothing but the in-process speedup, whereas letting the reader hit
    /// the instruction is a hard compile error.
    pub fn needs_eh_funclets(&self) -> bool {
        self.target_triple.contains("-windows-")
            && self.functions.iter().any(|f| f.personality.is_some())
    }

    /// Invoke-EH (#7302): declare the personality routine referenced by
    /// every `define ... personality ptr @perry_eh_personality`. Declared
    /// varargs — the symbol is only ever *named* on define lines and in the
    /// unwind tables; generated code never calls it.
    pub fn declare_personality(&mut self) {
        if self.declared_names.contains("perry_eh_personality") {
            return;
        }
        self.declared_names
            .insert("perry_eh_personality".to_string());
        self.declarations.push((
            "perry_eh_personality".to_string(),
            "declare i32 @perry_eh_personality(...)".to_string(),
        ));
    }

    /// Invoke-EH on windows-msvc (#7302): the SEH personality plus the
    /// module-local `__except` filter every catchpad names. The filter
    /// accepts exactly Perry's `RaiseException` code 0xE0504A53 ("PJS" |
    /// 0xE0000000, `perry-runtime/src/eh.rs`), so foreign SEH exceptions
    /// (access violations etc.) keep unwinding past JS handlers — the
    /// setjmp path never caught those either. Rendered among the
    /// declarations; LLVM accepts interleaved declares/defines.
    pub fn declare_seh_machinery(&mut self) {
        if self.declared_names.contains("__C_specific_handler") {
            return;
        }
        self.declared_names
            .insert("__C_specific_handler".to_string());
        self.declarations.push((
            "__C_specific_handler".to_string(),
            "declare i32 @__C_specific_handler(...)".to_string(),
        ));
        self.declared_names.insert("perry_seh_filter".to_string());
        self.declarations.push((
            "perry_seh_filter".to_string(),
            concat!(
                "define internal i32 @perry_seh_filter(ptr %eptrs, ptr %frame) {\n",
                "entry:\n",
                "  %rec = load ptr, ptr %eptrs\n",
                "  %code = load i32, ptr %rec\n",
                "  %ok = icmp eq i32 %code, -531609005\n",
                "  %r = zext i1 %ok to i32\n",
                "  ret i32 %r\n",
                "}"
            )
            .to_string(),
        ));
    }

    /// [`Self::declare_function`] with LLVM *return* parameter attributes
    /// (`nonnull`, `noalias`, …), which sit before the return type and so
    /// cannot be expressed through the trailing attribute-group string.
    ///
    /// Used for `js_shadow_frame_enter`, whose `nonnull` return is what lets
    /// LLVM fold away the null-state fallback arm that every inline shadow-slot
    /// store emits (#7088). The attribute is true by construction: the runtime
    /// returns the address of a `thread_local!`.
    pub fn declare_function_with_ret_attrs(
        &mut self,
        name: &str,
        return_type: LlvmType,
        param_types: &[LlvmType],
        ret_attrs: &str,
    ) {
        if self.declared_names.contains(name) {
            return;
        }
        self.declared_names.insert(name.to_string());
        let param_str = param_types.join(", ");
        let attrs = helper_decl_attrs(name);
        self.declarations.push((
            name.to_string(),
            format!(
                "declare {} {} @{}({}){}",
                ret_attrs, return_type, name, param_str, attrs
            ),
        ));
    }

    pub fn is_declared(&self, name: &str) -> bool {
        self.declared_names.contains(name)
    }

    /// Define (add) a function. Returns a mutable reference for block
    /// creation.
    pub fn define_function(
        &mut self,
        name: impl Into<String>,
        return_type: LlvmType,
        params: Vec<(LlvmType, String)>,
    ) -> &mut LlFunction {
        let func = LlFunction::new_with_fp_flags(name, return_type, params, self.fp_flags);
        self.functions.push(func);
        self.functions.last_mut().unwrap()
    }

    pub fn function_mut(&mut self, idx: usize) -> Option<&mut LlFunction> {
        self.functions.get_mut(idx)
    }

    /// Every defined function, mutably — for the whole-module passes that run
    /// after lowering and before any rendering path. See
    /// [`crate::root_reload`], and note that "before ANY rendering path" is the
    /// load-bearing part: the text renderer (`to_ir`, `render_codegen_units`)
    /// and the in-process constructor (`for_each_final_line`) are separate
    /// consumers, so a pass living inside one of them would silently not apply
    /// to the other.
    pub(crate) fn functions_mut(&mut self) -> impl Iterator<Item = &mut LlFunction> {
        self.functions.iter_mut()
    }

    /// Number of functions defined so far. Used to recover the index of a
    /// just-`define_function`ed function (whose `&mut` borrow must be released
    /// before the index can be read) when emitting a sequence of functions —
    /// e.g. the chunked string-pool init (#5391 function splitting).
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// True if a function with the given name has already been *defined*
    /// in this module. Used by the #461 export-stub pass to avoid
    /// redefining a symbol that an earlier emission path (function body,
    /// value-getter, #460 forwarding wrapper) already claimed.
    pub fn has_function(&self, name: &str) -> bool {
        self.functions.iter().any(|f| f.name == name)
    }

    pub fn add_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = global {} {}", name, ty, init));
    }

    pub fn add_external_global(&mut self, name: &str, ty: LlvmType) {
        self.globals
            .push(format!("@{} = external global {}", name, ty));
    }

    pub fn add_internal_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = internal global {} {}", name, ty, init));
    }

    /// Module-private read-only constant. Goes into `.rodata` instead of
    /// `.data` and the linker may merge identical copies across compilation
    /// units. Used by the ExternFuncRef-as-value path to emit static
    /// `ClosureHeader` records pointing at `__perry_wrap_extern_*` thunks
    /// — those are pure data and never mutated at runtime.
    pub fn add_internal_constant(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = internal constant {} {}", name, ty, init));
    }

    /// Push a fully-formed `@<name> = ...` line into the module's globals
    /// list. Used for constants whose type is not in the `LlvmType` enum
    /// (e.g. `[N x i32]` flat constant arrays for issue #50's folded
    /// module-level 2D int arrays).
    pub fn add_raw_global(&mut self, line: String) {
        self.globals.push(line);
    }

    /// Add a string constant with a caller-controlled name. Used by the
    /// `StringPool` so that emission order matches the pool's interned
    /// indices and the bytes globals can be referenced by name from
    /// `__perry_init_strings`.
    ///
    /// `escaped_lit` is the full LLVM IR literal *including* the surrounding
    /// `c"…"` and the trailing `\00`. `total_bytes` is the array length
    /// (= byte_len + 1 for the null terminator).
    pub fn add_named_string_constant(&mut self, name: &str, total_bytes: usize, escaped_lit: &str) {
        self.string_constants.push(format!(
            "@{} = private unnamed_addr constant [{} x i8] {}",
            name, total_bytes, escaped_lit
        ));
    }

    /// Add a UTF-8 string constant to the module's constant pool. Returns
    /// `(global_name, byte_length)` — the byte length is what Perry passes as
    /// the `len` argument to `js_string_from_bytes`.
    pub fn add_string_constant(&mut self, value: &str) -> (String, usize) {
        let name = format!(".str.{}", self.string_counter);
        self.string_counter += 1;

        let bytes = value.as_bytes();
        let len = bytes.len();
        let array_type = format!("[{} x i8]", len + 1);

        // Encode as an LLVM IR C-style string: printable ASCII pass through,
        // everything else becomes `\xx` hex escapes. Then append `\00` for
        // the C null terminator.
        let mut lit = String::with_capacity(len + 8);
        lit.push_str("c\"");
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");

        self.string_constants.push(format!(
            "@{} = private unnamed_addr constant {} {}",
            name, array_type, lit
        ));
        (name, len)
    }

    /// Functions to emit, each symbol AT MOST ONCE (first occurrence wins).
    ///
    /// Minified bundles can contain two distinct classes that sanitize to the
    /// same name (e.g. two classes `j`), producing colliding mangled method
    /// symbols (`perry_method_..._j__getElementsByTagName` defined twice). LLVM
    /// rejects the redefinition. Emitting each symbol once lets the module
    /// compile; calls to the duplicate resolve to the first definition (a
    /// dispatch ambiguity limited to genuinely name-colliding members — proper
    /// disambiguation by class id is a separate concern). Shared by [`to_ir`]
    /// and [`render_codegen_units`] so both paths agree on the symbol set.
    pub(crate) fn deduped_function_refs(&self) -> Vec<&LlFunction> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(self.functions.len());
        self.functions
            .iter()
            .filter(|f| seen.insert(f.name.as_str()))
            .collect()
    }

    /// The module *skeleton*: everything [`to_ir`] emits EXCEPT function
    /// definitions — header, string constants, globals, declarations (still
    /// filtered against defined names, which the native path adds via the C
    /// API), attribute groups and metadata.
    ///
    /// This is the only text the native construction path
    /// (`PERRY_LLVM_INPROCESS=native`) still parses: a few KB of module
    /// scaffolding, while every function body is built in memory. It must
    /// stay in lockstep with [`to_ir`] — both are thin loops over the same
    /// fields, and `native_emit`'s differential mode diffs the two paths'
    /// printed modules to catch drift.
    #[cfg(feature = "llvm-inprocess")]
    pub(crate) fn skeleton_ir(&self) -> String {
        let mut ir = String::new();
        ir.push_str("; Generated by perry-codegen\n");
        ir.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));
        for sc in &self.string_constants {
            ir.push_str(sc);
            ir.push('\n');
        }
        ir.push('\n');
        for g in &self.globals {
            ir.push_str(g);
            ir.push('\n');
        }
        ir.push('\n');
        let defined: HashSet<&str> = self
            .deduped_function_refs()
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        for (name, decl) in &self.declarations {
            if defined.contains(name.as_str()) {
                continue;
            }
            ir.push_str(decl);
            ir.push('\n');
        }
        ir.push('\n');
        self.push_attrs_and_metadata(&mut ir);
        ir
    }

    /// Serialize the module to a complete `.ll` file.
    pub fn to_ir(&self) -> String {
        let mut ir = String::new();
        ir.push_str("; Generated by perry-codegen\n");
        ir.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));
        if crate::codegen::helpers::native_stack_roots_enabled()
            && self.target_triple.contains("apple")
        {
            // LLVM emits one local `__LLVM_StackMaps` atom per object. Perry's
            // normal `-dead_strip` link otherwise discards those unreferenced
            // atoms. This Mach-O directive marks each local atom live without
            // globalizing the repeated symbol (which would collide across
            // codegen units).
            ir.push_str("module asm \".no_dead_strip __LLVM_StackMaps\"\n\n");
        }

        for sc in &self.string_constants {
            ir.push_str(sc);
            ir.push('\n');
        }
        ir.push('\n');

        for g in &self.globals {
            ir.push_str(g);
            ir.push('\n');
        }
        ir.push('\n');

        let funcs = self.deduped_function_refs();

        // Skip any `declare` whose name is also `define`d in this module —
        // LLVM rejects declare+define for the same symbol.
        let defined: HashSet<&str> = funcs.iter().map(|f| f.name.as_str()).collect();
        for (name, decl) in &self.declarations {
            if defined.contains(name.as_str()) {
                continue;
            }
            ir.push_str(decl);
            ir.push('\n');
        }
        if crate::codegen::helpers::native_stack_roots_enabled() {
            push_statepoint_declarations(&mut ir);
        }
        ir.push('\n');

        for func in &funcs {
            ir.push_str(&func.to_ir());
            ir.push('\n');
        }

        self.push_attrs_and_metadata(&mut ir);

        ir
    }

    /// Emit the shared setjmp attribute groups + the `!0`/buffer-alias metadata
    /// tail. Factored out of [`to_ir`] so each codegen unit can replicate the
    /// same attributes and metadata (so `#0`/`#1` and `!N` references resolve in
    /// every unit). Over-emitting an unused attribute group is harmless.
    fn push_attrs_and_metadata(&self, ir: &mut String) {
        // Verified runtime-helper groups (#6082) — emitted only when a
        // declaration actually references them (mirrors the setjmp gating
        // above). See `helper_decl_attrs` for the audit invariants.
        let mut used_pure = false;
        let mut used_readonly = false;
        let mut used_nounwind_willreturn = false;
        for name in &self.declared_names {
            match helper_decl_attrs(name) {
                " #2" => used_pure = true,
                " #3" => used_readonly = true,
                " #4" => used_nounwind_willreturn = true,
                _ => {}
            }
        }
        if used_pure {
            ir.push_str("\nattributes #2 = { nounwind willreturn readnone }\n");
        }
        if used_readonly {
            ir.push_str("\nattributes #3 = { nounwind willreturn readonly }\n");
        }
        if used_nounwind_willreturn {
            ir.push_str("\nattributes #4 = { nounwind willreturn }\n");
        }
        // Issue #52: `!0 = !{}` referenced by `!invariant.load !0`, plus the
        // buffer alias-scope metadata. LICM/GVN hoist invariant loads out of
        // loops only with these present.
        ir.push_str("\n!0 = !{}\n");
        for ml in &self.metadata_lines {
            ir.push_str(ml);
            ir.push('\n');
        }
    }

    /// Render this module as `n` independent codegen-unit `.ll` texts (#5391).
    ///
    /// Each unit is independently compilable by `clang -c`, so peak compiler
    /// memory is bounded to ~1/n of the whole module — the structural fix for
    /// the single giant translation unit that makes clang OOM on large bundles.
    ///
    /// The functions are split into `n` contiguous buckets. Every unit carries:
    ///   * the full string-constant + global set, with local-linkage and bare
    ///     external DEFINITIONS promoted to `linkonce_odr` (the linker keeps one
    ///     copy). Globals are a tiny fraction of a large module's IR, so the
    ///     duplication is cheap; `external` *declarations* are replicated as-is;
    ///   * the module's external `declare`s plus a synthesized `declare` for
    ///     every locally-defined function the unit does NOT itself define, so
    ///     cross-unit calls resolve at link time (deduped by name, existing
    ///     declarations win);
    ///   * each function rendered with external linkage forced (the lone
    ///     `internal` init/wrapper is promoted so cross-unit calls bind);
    ///   * the shared attribute groups + metadata (so `#N`/`!N` refs resolve).
    ///
    /// `n <= 1` (or a single-function module) returns a single part whose
    /// `funcs` are all functions (callers use the whole-module path). The
    /// text caller compiles each rendered part and combines them (`ld -r`)
    /// into one object, keeping `compile_module`'s single-object API.
    pub(crate) fn codegen_unit_parts(&self, n: usize) -> Vec<CodegenUnitPart<'_>> {
        let funcs = self.deduped_function_refs();
        if n <= 1 || funcs.len() <= 1 {
            return vec![CodegenUnitPart {
                pre: String::new(),
                post: String::new(),
                funcs,
            }];
        }
        let n = n.min(funcs.len());

        // Balance units by estimated byte size, not function count: minified
        // bundles have a few enormous functions (a 68MB IIFE in the cli.js
        // case), so contiguous count-chunking can clump them into one outsized
        // unit whose clang -O0 time dominates. Greedy largest-first bin-packing
        // assigns each function to the currently-smallest unit, isolating big
        // functions and keeping the rest even. (A single function larger than
        // total/n is irreducible here — that is the intra-function #4880
        // problem, not something inter-function splitting can divide.)
        let sizes: Vec<usize> = funcs.iter().map(|f| f.estimated_ir_bytes()).collect();
        let mut order: Vec<usize> = (0..funcs.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
        let mut buckets: Vec<Vec<&LlFunction>> = vec![Vec::new(); n];
        let mut bucket_bytes = vec![0usize; n];
        for &i in &order {
            let target = bucket_bytes
                .iter()
                .enumerate()
                .min_by_key(|&(_, &b)| b)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            buckets[target].push(funcs[i]);
            bucket_bytes[target] += sizes[i];
        }

        let shared_strings: Vec<String> = self
            .string_constants
            .iter()
            .map(|s| promote_global_for_units(s))
            .collect();
        let shared_globals: Vec<String> = self
            .globals
            .iter()
            .map(|g| promote_global_for_units(g))
            .collect();

        // name -> declare line. Existing module declarations (runtime, FFI,
        // cross-module) take precedence; every locally-defined function without
        // one gets a synthesized declare. Deduped by name so no unit emits a
        // duplicate declaration. BTreeMap for deterministic unit output.
        let mut decl_by_name: BTreeMap<&str, String> = BTreeMap::new();
        for (name, decl) in &self.declarations {
            decl_by_name.insert(name.as_str(), decl.clone());
        }
        for f in &funcs {
            decl_by_name
                .entry(f.name.as_str())
                .or_insert_with(|| declare_line_for(f));
        }

        // #7174 (real-app scaling): render each bucket's functions first, then
        // give every global/string exactly ONE defining unit and hand the rest
        // an `external` declaration. Replicating all definitions into every
        // unit made per-unit IR grow with unit COUNT — on the 13 MB Claude Code
        // bundle that meant ~400 MB units and `clang: translation unit is too
        // large ... ran out of source locations`, no matter how finely it was
        // split. Definitions are already `linkonce_odr` (visible), so an
        // external declaration resolves to the same symbol at link time.
        let bucket_texts: Vec<String> = buckets
            .iter()
            .map(|bucket| {
                let mut t = String::new();
                for func in bucket {
                    t.push_str(&render_fn_external(func));
                    t.push('\n');
                }
                t
            })
            .collect();
        let bucket_refs: Vec<HashSet<String>> = bucket_texts
            .iter()
            .map(|t| {
                let mut refs = HashSet::new();
                collect_symbol_refs(t, &mut refs);
                refs
            })
            .collect();

        // A global is emitted into every unit that REFERENCES it — normally
        // exactly one, and `linkonce_odr` lets the linker fold the rare
        // multi-unit case. Definition-in-one-unit + `external` elsewhere was
        // tried first and is subtly wrong under `-dead_strip`: the sole
        // definition can be discarded with its unit's atoms while a live
        // reference survives in another object.
        let all_globals: Vec<&String> =
            shared_strings.iter().chain(shared_globals.iter()).collect();
        // Globals reference OTHER globals in their initializers (a string
        // header pointing at its `.bytes` payload, a closure record naming its
        // thunk). Function-text references alone therefore under-approximate
        // what a unit needs — the first cut emitted `@....str.N.bytes` nowhere
        // and clang rejected the unit with "use of undefined value". Close the
        // reference set transitively per unit before deciding what to emit.
        let global_index: std::collections::HashMap<&str, usize> = all_globals
            .iter()
            .enumerate()
            .filter_map(|(i, def)| global_symbol_name(def).map(|nm| (nm, i)))
            .collect();
        let global_refs: Vec<HashSet<String>> = all_globals
            .iter()
            .map(|def| {
                let mut refs = HashSet::new();
                collect_symbol_refs(def, &mut refs);
                refs
            })
            .collect();
        let bucket_needs: Vec<HashSet<usize>> = bucket_refs
            .iter()
            .map(|refs| {
                let mut need: HashSet<usize> = refs
                    .iter()
                    .filter_map(|nm| global_index.get(nm.as_str()).copied())
                    .collect();
                let mut work: Vec<usize> = need.iter().copied().collect();
                while let Some(gi) = work.pop() {
                    for nm in &global_refs[gi] {
                        if let Some(&next) = global_index.get(nm.as_str()) {
                            if need.insert(next) {
                                work.push(next);
                            }
                        }
                    }
                }
                need
            })
            .collect();
        let referenced_anywhere: Vec<bool> = (0..all_globals.len())
            .map(|gi| bucket_needs.iter().any(|need| need.contains(&gi)))
            .collect();

        let mut post = String::new();
        self.push_attrs_and_metadata(&mut post);

        let mut parts = Vec::with_capacity(n);
        for (bi, bucket) in buckets.into_iter().enumerate() {
            let defined: HashSet<&str> = bucket.iter().map(|f| f.name.as_str()).collect();
            let mut pre = String::new();
            pre.push_str("; Generated by perry-codegen (codegen unit)\n");
            pre.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));
            if crate::codegen::helpers::native_stack_roots_enabled()
                && self.target_triple.contains("apple")
            {
                pre.push_str("module asm \".no_dead_strip __LLVM_StackMaps\"\n\n");
            }

            for (gi, def) in all_globals.iter().enumerate() {
                let referenced = bucket_needs[bi].contains(&gi);
                // Unreferenced globals (anchors, `llvm.*`, appending lists)
                // keep a home in unit 0 so nothing is lost.
                if referenced || (!referenced_anywhere[gi] && bi == 0) {
                    pre.push_str(def);
                    pre.push('\n');
                }
            }
            pre.push('\n');

            // Declares for everything this unit REFERENCES but does not
            // define. Emitting the whole module's declaration list into every
            // unit left a per-unit floor that splitting cannot reduce: a
            // 24-function benchmark carried 2,972 declares (149 KB) per unit,
            // and the 13 MB Claude Code bundle carried ~16,700 — which is how
            // units stayed above a gigabyte and hit clang's 2^31 source-location
            // ceiling ("translation unit is too large ... ran out of source
            // locations") regardless of unit count. Referenced names include
            // those reached through the initializers of the globals this unit
            // emits, so the closure computed above feeds this filter too.
            // `collect_symbol_refs` yields `@name`; `decl_by_name` is keyed on
            // the bare name, so strip the sigil or nothing ever matches.
            let mut needed: HashSet<&str> = bucket_refs[bi]
                .iter()
                .map(|nm| nm.trim_start_matches('@'))
                .collect();
            for gi in &bucket_needs[bi] {
                for nm in &global_refs[*gi] {
                    needed.insert(nm.trim_start_matches('@'));
                }
            }
            for (name, decl) in &decl_by_name {
                if defined.contains(name) || !needed.contains(*name) {
                    continue;
                }
                pre.push_str(decl);
                pre.push('\n');
            }
            if crate::codegen::helpers::native_stack_roots_enabled() {
                push_statepoint_declarations(&mut pre);
            }
            pre.push('\n');

            parts.push(CodegenUnitPart {
                pre,
                post: post.clone(),
                funcs: bucket,
            });
        }
        parts
    }

    /// Render this module as `n` independent codegen-unit `.ll` texts (#5391).
    /// Thin text renderer over [`codegen_unit_parts`]; the native construction
    /// path consumes the parts directly.
    pub fn render_codegen_units(&self, n: usize) -> Vec<String> {
        let parts = self.codegen_unit_parts(n);
        if parts.len() == 1 {
            return vec![self.to_ir()];
        }
        parts
            .into_iter()
            .map(|part| {
                let mut ir = part.pre;
                for func in &part.funcs {
                    ir.push_str(&render_fn_external(func));
                    ir.push('\n');
                }
                ir.push_str(&part.post);
                ir
            })
            .collect()
    }
}

/// One codegen unit, pre-render: the textual skeleton around the functions
/// (`pre` = header/strings/globals/cross-unit declares; `post` = shared
/// attribute groups + metadata) plus the functions themselves, un-rendered so
/// the native backend can construct them directly.
pub(crate) struct CodegenUnitPart<'m> {
    pub pre: String,
    pub post: String,
    pub funcs: Vec<&'m LlFunction>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DOUBLE, I32, I64, PTR, VOID};

    #[test]
    fn render_codegen_units_partitions_and_links() {
        // #5391: a 2-unit split of a 2-function module must (a) define each
        // function in exactly one unit, (b) declare the other so cross-unit
        // calls resolve, and (c) carry the shared globals in BOTH units with
        // local linkage promoted to linkonce_odr (linker dedups).
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
        m.add_internal_global("perry_global_x", DOUBLE, "0.0");
        let (_s, _l) = m.add_string_constant("hi");

        // f() calls g()
        let f = m.define_function("perry_fn_m__f", DOUBLE, vec![]);
        let e = f.create_block("entry");
        let r = e.call(DOUBLE, "perry_fn_m__g", &[]);
        e.ret(DOUBLE, &r);
        let g = m.define_function("perry_fn_m__g", DOUBLE, vec![]);
        let e2 = g.create_block("entry");
        e2.ret(DOUBLE, "0.0");

        let units = m.render_codegen_units(2);
        assert_eq!(units.len(), 2, "two functions → two units");

        // Each function defined exactly once across all units.
        let def_f = units
            .iter()
            .filter(|u| u.contains("define double @perry_fn_m__f("))
            .count();
        let def_g = units
            .iter()
            .filter(|u| u.contains("define double @perry_fn_m__g("))
            .count();
        assert_eq!(def_f, 1);
        assert_eq!(def_g, 1);

        // The unit that DEFINES f (and calls g) must DECLARE g.
        let u_with_f = units
            .iter()
            .find(|u| u.contains("define double @perry_fn_m__f("))
            .unwrap();
        assert!(u_with_f.contains("declare double @perry_fn_m__g()"));

        // #7174: each shared global is DEFINED exactly once across units;
        // units that reference it get an `external` declaration instead of a
        // copy. Replicating definitions made per-unit IR grow with the unit
        // count and broke clang's translation-unit limit on real bundles.
        let global_defs = units
            .iter()
            .filter(|u| u.contains("@perry_global_x = linkonce_odr global double 0.0"))
            .count();
        assert_eq!(global_defs, 1, "global must be defined in exactly one unit");
        let str_defs = units
            .iter()
            .filter(|u| u.contains("@.str.0 = linkonce_odr unnamed_addr constant"))
            .count();
        assert_eq!(str_defs, 1, "string must be defined in exactly one unit");

        // Every unit that mentions the symbol either defines it or declares it
        // external — never neither.
        for u in &units {
            if u.contains("@perry_global_x") {
                assert!(
                    u.contains("@perry_global_x = linkonce_odr global double 0.0")
                        || u.contains("@perry_global_x = external global double"),
                    "referencing unit must define or externally declare the global"
                );
            }
            // Declares are now scoped to what a unit references (the
            // whole-module declaration list was a per-unit floor that
            // splitting could not reduce). A unit that calls the helper must
            // still declare it.
            if u.contains("call void @js_console_log_number") {
                assert!(
                    u.contains("declare void @js_console_log_number(double)"),
                    "a unit calling the helper must declare it"
                );
            }
            assert!(u.contains("target triple = \"arm64-apple-macosx15.0.0\""));
        }
    }

    #[test]
    fn duplicate_function_symbol_emitted_once() {
        // Two classes that sanitize to the same name produce a colliding
        // method symbol; it must be emitted once (LLVM rejects redefinition),
        // in both the single-TU and the codegen-unit render paths.
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        for _ in 0..2 {
            let f = m.define_function("perry_method_j__foo", DOUBLE, vec![]);
            f.create_block("entry").ret(DOUBLE, "0.0");
        }
        assert_eq!(
            m.to_ir()
                .matches("define double @perry_method_j__foo(")
                .count(),
            1,
            "duplicate symbol must be defined once in to_ir"
        );
        let units = m.render_codegen_units(4);
        let defs: usize = units
            .iter()
            .map(|u| u.matches("define double @perry_method_j__foo(").count())
            .sum();
        assert_eq!(
            defs, 1,
            "duplicate symbol must be defined once across units"
        );
    }

    #[test]
    fn render_codegen_units_balances_by_size_isolating_a_giant_fn() {
        // One huge function + several tiny ones, split into 2 units: greedy
        // size bin-packing must isolate the giant function so it does NOT share
        // a unit with the tiny ones (which would make that unit outsized).
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        let big = m.define_function("perry_fn_m__big", DOUBLE, vec![]);
        let be = big.create_block("entry");
        for _ in 0..2000 {
            be.call_void("js_noop", &[]);
        }
        be.ret(DOUBLE, "0.0");
        for k in 0..6 {
            let f = m.define_function(format!("perry_fn_m__small{k}"), DOUBLE, vec![]);
            f.create_block("entry").ret(DOUBLE, "0.0");
        }
        let units = m.render_codegen_units(2);
        assert_eq!(units.len(), 2);
        let big_unit = units
            .iter()
            .find(|u| u.contains("define double @perry_fn_m__big("))
            .unwrap();
        // The giant function's unit holds (essentially) only it — the six small
        // functions land in the other unit to balance bytes.
        let smalls_with_big = (0..6)
            .filter(|k| big_unit.contains(&format!("define double @perry_fn_m__small{k}(")))
            .count();
        assert!(
            smalls_with_big <= 1,
            "giant function should be isolated, not clumped with the small ones (got {smalls_with_big})"
        );
    }

    #[test]
    fn render_codegen_units_single_unit_matches_to_ir() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        let f = m.define_function("main", I32, vec![]);
        f.create_block("entry").ret(I32, "0");
        assert_eq!(m.render_codegen_units(1), vec![m.to_ir()]);
    }

    #[test]
    fn helper_attr_groups_on_verified_declarations_only() {
        // #6082: allowlisted helpers carry the #2 (pure) / #3 (readonly)
        // group refs; a non-allowlisted helper (js_nanbox_string ALLOCATES)
        // must not; each attributes line is emitted exactly once.
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_nanbox_get_pointer", I64, &[DOUBLE]);
        m.declare_function("js_is_truthy", I32, &[DOUBLE]);
        m.declare_function("js_nanbox_string", DOUBLE, &[I64]);
        m.declare_function(
            "js_typed_feedback_numeric_array_index_get_guard",
            I32,
            &[I64, DOUBLE, I32, I32],
        );
        let f = m.define_function("main", I32, vec![]);
        f.create_block("entry").ret(I32, "0");

        let ir = m.to_ir();
        assert!(
            ir.contains("declare i64 @js_nanbox_get_pointer(double) #2"),
            "pure helper must carry the #2 group ref"
        );
        assert!(
            ir.contains("declare i32 @js_is_truthy(double) #3"),
            "readonly helper must carry the #3 group ref"
        );
        assert!(
            ir.contains("declare double @js_nanbox_string(i64)\n"),
            "allocating helper must stay attribute-free"
        );
        assert!(!ir.contains("js_nanbox_string(i64) #"));
        assert_eq!(
            ir.matches("attributes #2 = { nounwind willreturn readnone }")
                .count(),
            1
        );
        assert_eq!(
            ir.matches("attributes #3 = { nounwind willreturn readonly }")
                .count(),
            1
        );
        // Repsel 4a.0: the array-index guards carry #4 (nounwind willreturn,
        // no memory attribute — the first-touch path rebuilds raw-f64 layout).
        assert!(ir.contains(
            "declare i32 @js_typed_feedback_numeric_array_index_get_guard(i64, double, i32, i32) #4"
        ));
        assert_eq!(
            ir.matches("attributes #4 = { nounwind willreturn }")
                .count(),
            1
        );
        // No setjmp declared → the setjmp-only groups stay out.
        assert!(!ir.contains("attributes #0"));
        assert!(!ir.contains("attributes #1"));
    }

    #[test]
    fn helper_attr_groups_omitted_when_unused() {
        // A module that declares no allowlisted helper must not emit the
        // #2/#3 attributes lines (mirrors the setjmp #0/#1 gating).
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
        let f = m.define_function("main", I32, vec![]);
        f.create_block("entry").ret(I32, "0");
        let ir = m.to_ir();
        assert!(!ir.contains("attributes #2"));
        assert!(!ir.contains("attributes #3"));
        assert!(!ir.contains("attributes #4"));
    }

    #[test]
    fn helper_attr_groups_replicated_in_codegen_units() {
        // Every codegen unit re-emits the declaration (with its group ref)
        // and the attributes line, so #2/#3 references resolve per-unit.
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_is_truthy", I32, &[DOUBLE]);
        for k in 0..2 {
            let f = m.define_function(format!("perry_fn_m__f{k}"), DOUBLE, vec![]);
            let b = f.create_block("entry");
            // Reference the helper so the declare is genuinely needed: declares
            // are scoped per unit now, and a test whose units never call the
            // helper would assert nothing about its attribute group.
            b.call(I32, "js_is_truthy", &[(DOUBLE, "0.0")]);
            b.ret(DOUBLE, "0.0");
        }
        let units = m.render_codegen_units(2);
        assert_eq!(units.len(), 2);
        for u in &units {
            assert!(u.contains("declare i32 @js_is_truthy(double) #3"));
            assert_eq!(
                u.matches("attributes #3 = { nounwind willreturn readonly }")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn hello_world_ir_is_well_formed() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
        let (_sname, _slen) = m.add_string_constant("hello");

        let f = m.define_function("main", I32, vec![]);
        let entry = f.create_block("entry");
        entry.call_void("js_console_log_number", &[(DOUBLE, "42.0")]);
        entry.ret(I32, "0");

        let ir = m.to_ir();
        assert!(ir.contains("target triple = \"arm64-apple-macosx15.0.0\""));
        assert!(ir.contains("declare void @js_console_log_number(double)"));
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("call void @js_console_log_number(double 42.0)"));
        assert!(ir.contains("ret i32 0"));
    }

    #[test]
    fn declare_is_dropped_when_also_defined() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("main", I32, &[]);
        let f = m.define_function("main", I32, vec![]);
        f.create_block("entry").ret(I32, "0");
        let ir = m.to_ir();
        assert!(!ir.contains("declare i32 @main"));
        assert!(ir.contains("define i32 @main"));
    }

    #[test]
    fn string_constant_escapes_nonprintable() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        let (name, len) = m.add_string_constant("a\nb");
        assert_eq!(name, ".str.0");
        assert_eq!(len, 3);
        let ir = m.to_ir();
        // "a" then \0A then "b" then \00
        assert!(ir.contains("c\"a\\0Ab\\00\""), "got: {}", ir);
    }

    #[test]
    fn gep_unused_helper_imports_compile() {
        // Smoke test that PTR, I64 are re-exported and compile alongside.
        let _ = (PTR, I64);
    }
}
