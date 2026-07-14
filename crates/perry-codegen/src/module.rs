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
/// * `#2` (PURE) = `nounwind willreturn memory(none)`. Invariant: the
///   helper's Rust body (transitively) performs NaN-box BIT manipulation
///   only — no loads, no stores, no allocation, no GC trigger, no
///   `js_throw`/longjmp, and it is total over arbitrary input bits (no
///   panic, no UB), so LLVM may CSE/hoist/sink/delete it freely.
/// * `#3` (READONLY) = `nounwind willreturn memory(read)`. Invariant: the
///   helper may READ heap memory (string headers, BigInt limbs) but never
///   writes, never allocates, never triggers GC, never takes a lock, and
///   never throws. LLVM may CSE/LICM it across write-free regions and
///   delete unused calls, but must still order it against any
///   possibly-writing call — which keeps it correct w.r.t. the moving GC,
///   because every GC-capable helper stays maximally clobbering.
///
/// SOUNDNESS NOTES (read before adding an entry):
/// * Deliberately `memory(read)`, NOT `memory(argmem: read)`: helper args
///   are f64 NaN-boxes, not LLVM pointer arguments, so `argmem` would mean
///   "reads no memory at all" and license CSE/DSE across real heap reads.
/// * Anything that can allocate or trigger GC gets NO group — the moving
///   GC's shadow-stack reload discipline depends on those calls staying
///   maximally clobbering.
/// * Anything that can reach `js_throw` (setjmp/longjmp) gets NO group —
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
fn helper_decl_attrs(name: &str) -> &'static str {
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
        _ => "",
    }
}

/// Synthesize an external `declare` line matching a locally-defined function's
/// signature, so a codegen unit that calls it (but does not define it) resolves
/// the call at link time.
fn declare_line_for(f: &LlFunction) -> String {
    let params = f
        .params
        .iter()
        .map(|(t, _)| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let attrs = if f.name == "setjmp" || f.name == "_setjmp" {
        " #0"
    } else {
        ""
    };
    format!("declare {} @{}({}){}", f.return_type, f.name, params, attrs)
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
        // setjmp needs the `returns_twice` attribute to prevent
        // LLVM from promoting alloca slots to SSA registers across
        // the setjmp boundary. Without it, local variables modified
        // between setjmp and longjmp are clobbered when the second
        // return (via longjmp) happens.
        //
        // Verified-pure runtime helpers get the #2/#3 optimization groups
        // (#6082) — see `helper_decl_attrs` for the audit invariants. The
        // lookup is name-keyed here in the single declaration funnel so
        // every declaration path agrees on the attributes.
        let attrs = if name == "setjmp" || name == "_setjmp" {
            " #0"
        } else {
            helper_decl_attrs(name)
        };
        self.declarations.push((
            name.to_string(),
            format!("declare {} @{}({}){}", return_type, name, param_str, attrs),
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
    fn deduped_function_refs(&self) -> Vec<&LlFunction> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(self.functions.len());
        self.functions
            .iter()
            .filter(|f| seen.insert(f.name.as_str()))
            .collect()
    }

    /// Serialize the module to a complete `.ll` file.
    pub fn to_ir(&self) -> String {
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
        // Attribute group for setjmp's `returns_twice` marker. Only emit if
        // setjmp (any variant) was declared. Apple declares `_setjmp`, Windows
        // `_setjmp` (2-arg ABI), Linux `setjmp` — all need `returns_twice`.
        if self.declared_names.contains("setjmp") || self.declared_names.contains("_setjmp") {
            ir.push_str("\nattributes #0 = { returns_twice }\n");
            // Functions containing a `try` are marked `#1`.
            //
            // This group used to carry `optnone` as well, to stop mem2reg/SROA
            // from promoting allocas across the setjmp call (a promoted local
            // lives in a callee-saved register, which `longjmp` restores to its
            // setjmp-time value — so try-body mutations were invisible to the
            // catch). That worked, but it deoptimized the ENTIRE function: just
            // having a `try` cost ~5x on the surrounding loop even when nothing
            // ever threw (#6385).
            //
            // The promotion is now blocked surgically instead, by emitting
            // `volatile` loads/stores for exactly the allocas the try body
            // writes (see `crate::volatile_setjmp`) — LLVM refuses to promote
            // an alloca with any volatile access. Everything else optimizes.
            //
            // `noinline` stays. LLVM's `isInlineViable` already refuses to
            // inline a function that contains a `returns_twice` call, so this
            // is belt-and-braces rather than load-bearing — but it keeps the
            // setjmp frame's identity from depending on an internal inliner
            // policy, at zero cost.
            ir.push_str("attributes #1 = { noinline }\n");
        }
        // Verified runtime-helper groups (#6082) — emitted only when a
        // declaration actually references them (mirrors the setjmp gating
        // above). See `helper_decl_attrs` for the audit invariants.
        let mut used_pure = false;
        let mut used_readonly = false;
        for name in &self.declared_names {
            match helper_decl_attrs(name) {
                " #2" => used_pure = true,
                " #3" => used_readonly = true,
                _ => {}
            }
        }
        if used_pure {
            ir.push_str("\nattributes #2 = { nounwind willreturn memory(none) }\n");
        }
        if used_readonly {
            ir.push_str("\nattributes #3 = { nounwind willreturn memory(read) }\n");
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
    /// `n <= 1` (or a single-function module) returns one text identical to
    /// [`to_ir`]. The caller compiles each text to an object and combines them
    /// (`ld -r`) into one object, keeping `compile_module`'s single-object API.
    pub fn render_codegen_units(&self, n: usize) -> Vec<String> {
        let funcs = self.deduped_function_refs();
        if n <= 1 || funcs.len() <= 1 {
            return vec![self.to_ir()];
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

        let mut units = Vec::with_capacity(n);
        for bucket in &buckets {
            let defined: HashSet<&str> = bucket.iter().map(|f| f.name.as_str()).collect();
            let mut ir = String::new();
            ir.push_str("; Generated by perry-codegen (codegen unit)\n");
            ir.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));

            for sc in &shared_strings {
                ir.push_str(sc);
                ir.push('\n');
            }
            ir.push('\n');
            for g in &shared_globals {
                ir.push_str(g);
                ir.push('\n');
            }
            ir.push('\n');

            // Declares for everything this unit references but does not define.
            for (name, decl) in &decl_by_name {
                if defined.contains(name) {
                    continue;
                }
                ir.push_str(decl);
                ir.push('\n');
            }
            ir.push('\n');

            for func in bucket {
                ir.push_str(&render_fn_external(func));
                ir.push('\n');
            }

            self.push_attrs_and_metadata(&mut ir);
            units.push(ir);
        }
        units
    }
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

        // Shared globals appear in BOTH units, promoted to linkonce_odr.
        for u in &units {
            assert!(u.contains("@perry_global_x = linkonce_odr global double 0.0"));
            assert!(u.contains("@.str.0 = linkonce_odr unnamed_addr constant"));
            assert!(u.contains("declare void @js_console_log_number(double)"));
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
            ir.matches("attributes #2 = { nounwind willreturn memory(none) }")
                .count(),
            1
        );
        assert_eq!(
            ir.matches("attributes #3 = { nounwind willreturn memory(read) }")
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
    }

    #[test]
    fn helper_attr_groups_replicated_in_codegen_units() {
        // Every codegen unit re-emits the declaration (with its group ref)
        // and the attributes line, so #2/#3 references resolve per-unit.
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_is_truthy", I32, &[DOUBLE]);
        for k in 0..2 {
            let f = m.define_function(format!("perry_fn_m__f{k}"), DOUBLE, vec![]);
            f.create_block("entry").ret(DOUBLE, "0.0");
        }
        let units = m.render_codegen_units(2);
        assert_eq!(units.len(), 2);
        for u in &units {
            assert!(u.contains("declare i32 @js_is_truthy(double) #3"));
            assert_eq!(
                u.matches("attributes #3 = { nounwind willreturn memory(read) }")
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
