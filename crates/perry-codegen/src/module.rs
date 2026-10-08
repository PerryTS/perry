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

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::block::FpFlags;
use crate::function::LlFunction;
use crate::native_value::NativeRepRecord;
use crate::types::LlvmType;

mod linkage;
pub(crate) use linkage::*;

#[cfg(test)]
mod unit_partition_tests;

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
    /// Per-compile module-state ownership, copied from CompileOptions.
    pub(crate) program_has_worker: bool,
    pub target_triple: String,
    declarations: Vec<(String, String)>, // (name, full "declare …" line)
    declared_names: HashSet<String>,
    functions: Vec<LlFunction>,
    defined_names: HashSet<String>,
    globals: Vec<String>,
    string_constants: Vec<String>,
    string_counter: u32,
    /// Module symbol prefix folded into every anonymous rodata constant this
    /// module mints (`add_string_constant` → `@<prefix>_.str.N`). Empty (the
    /// bare `@.str.N`) only for modules that never call
    /// [`Self::set_symbol_prefix`] — unit tests and other single-module
    /// fixtures.
    ///
    /// Load-bearing under codegen-unit splitting: `render_codegen_units`
    /// promotes every `private` constant so sibling units can reference it,
    /// and on ELF/COFF the owning unit's copy is a plain STRONG global. The
    /// `.str.N` counter restarts at 0 per module, so two split modules used to
    /// export the same `.str.375` with different contents — GNU ld rejects
    /// that as a multiple definition, and ld64's weak coalescing silently kept
    /// whichever copy it saw first. The prefix makes the name module-unique,
    /// exactly as `strings.rs` already does for `<prefix>_.str.N.bytes`.
    symbol_prefix: String,
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
    /// #8175: symbols that must be defined, declared, AND called with the
    /// `preserve_nonecc` calling convention — recursion-participating
    /// specialized clones. One shared cell, injected into every function's
    /// `RegCounter` at `define_function` time; population happens once, after
    /// the specialization plan is final (`codegen/mod.rs`), and reads happen
    /// at call-emission/render time, so define order never matters.
    preserve_none_fns: Rc<RefCell<HashSet<String>>>,
    /// The module's `JsFunctionInfo` requests and facts (`crate::fn_info`),
    /// shared with every function's register counter; emitted by
    /// [`Self::emit_fn_infos`] once every function exists.
    fn_infos: Rc<RefCell<crate::fn_info::FnInfoState>>,
    /// Whether this module's own thread-locals use the local-exec model
    /// ([`Self::use_local_exec_tls`]). Module state: the output kind of the
    /// compile that built this module decides it, never a process global.
    local_exec_tls: bool,
}

/// The `source_filename` every Perry-emitted module records.
///
/// Without it, LLVM records whatever path the caller handed the assembler. The
/// textual pipeline writes each module to a per-call temp file
/// (`perry_llvm_<nonce>.ll`), so the recorded name carried a random nonce,
/// while native construction recorded its in-memory module id instead. ELF
/// stores that name as an `STT_FILE` symbol, so the two construction paths
/// could never produce byte-identical objects and neither was reproducible
/// across runs. Mach-O records no such symbol, which is why this was invisible
/// on macOS hosts and only ever failed on Linux (#8087).
pub(crate) const MODULE_SOURCE_NAME: &str = "perry_module";

impl LlModule {
    pub(crate) fn declaration_lines(&self) -> impl Iterator<Item = (&str, &str)> {
        self.declarations
            .iter()
            .map(|(name, line)| (name.as_str(), line.as_str()))
    }

    /// #10399: names of the globals this module DEFINES thread-local.
    ///
    /// The TLS specifier is part of a symbol's identity. A stale non-TLS
    /// `external` declaration for one of these — import metadata can register
    /// one before the defining pass runs — makes the linker reject the object
    /// with "TLS definition ... mismatches non-TLS reference". The native
    /// unit path pushes the whole declaration table into every unit, so the
    /// table itself has to agree with the definitions.
    pub(crate) fn thread_local_global_names(&self) -> std::collections::HashSet<String> {
        self.globals
            .iter()
            .filter(|g| g.contains(" thread_local ") || g.contains(" thread_local("))
            .filter_map(|g| global_symbol_name(g).map(|s| s.trim_start_matches('@').to_string()))
            .collect()
    }

    pub fn new(target_triple: impl Into<String>) -> Self {
        Self::new_with_fp_flags(target_triple, FpFlags::default())
    }

    pub fn new_with_fp_flags(target_triple: impl Into<String>, fp_flags: FpFlags) -> Self {
        Self {
            target_triple: target_triple.into(),
            program_has_worker: false,
            declarations: Vec::new(),
            declared_names: HashSet::new(),
            functions: Vec::new(),
            defined_names: HashSet::new(),
            globals: Vec::new(),
            string_constants: Vec::new(),
            string_counter: 0,
            symbol_prefix: String::new(),
            metadata_lines: Vec::new(),
            ic_counter: 0,
            buffer_alias_counter: 0,
            native_rep_records: Vec::new(),
            fp_flags,
            preserve_none_fns: Rc::new(RefCell::new(HashSet::new())),
            fn_infos: Rc::new(RefCell::new(crate::fn_info::FnInfoState::default())),
            local_exec_tls: false,
        }
    }

    /// Address the module's own thread-locals as fixed offsets from the
    /// thread pointer: the local-exec model, exact only in an ELF executable,
    /// whose TLS block is the static one every thread starts with.
    ///
    /// Module state is thread-local when the program starts a Worker (#10399),
    /// and the units are compiled position-independent, where LLVM must assume
    /// a thread-local can live in any loaded image: every access became a
    /// general-dynamic `__tls_get_addr` call that the register allocator
    /// treated as a call clobbering every caller-saved register.
    ///
    /// Every definition and declaration still at the default
    /// (general-dynamic) model is rewritten; a runtime thread-local declared
    /// with the model its definition needs (`PERRY_AGENT_PTRS` is
    /// `initialexec`) keeps it. The model is written into the module itself,
    /// so the IR text (and the object-cache key derived from it) states it,
    /// and an emitter running on another thread needs no outside input.
    pub(crate) fn use_local_exec_tls(&mut self) {
        self.local_exec_tls = true;
        let lines = self
            .globals
            .iter_mut()
            .chain(self.declarations.iter_mut().map(|(_, line)| line));
        for line in lines {
            if let Some(rewritten) = crate::module::linkage::with_local_exec_tls(line) {
                *line = rewritten;
            }
        }
    }

    /// The TLS specifier a declaration this module adds later must carry to
    /// agree with its definitions ([`Self::use_local_exec_tls`]).
    pub(crate) fn thread_local_specifier(&self) -> &'static str {
        if self.local_exec_tls {
            "thread_local(localexec)"
        } else {
            "thread_local"
        }
    }

    /// Install the per-module symbol prefix that [`Self::add_string_constant`]
    /// folds into every anonymous constant it mints. Must run before the
    /// first string constant is added — a prefix that only covers part of
    /// the pool would leave the earlier `.str.N` names colliding across
    /// modules again.
    pub fn set_symbol_prefix(&mut self, prefix: &str) {
        debug_assert!(
            self.string_constants.is_empty() && self.functions.is_empty(),
            "set_symbol_prefix must precede the first add_string_constant/define_function"
        );
        self.symbol_prefix = prefix.to_string();
    }

    /// Name (no `@`) of this module's null-guard global — the zeroed `i32`
    /// that `LlBlock::safe_load_i32_from_ptr` reads instead of a bad handle.
    /// The caller defines it (`add_internal_global(.., I32, "0")`); every
    /// function defined afterwards references it by this name. Module-prefixed
    /// for the same reason as `add_string_constant`'s names: unit splitting
    /// promotes it to a strong link-visible symbol on ELF/COFF, and the bare
    /// `perry_null_guard_zero` in two split modules is a GNU ld
    /// `multiple definition`.
    pub fn null_guard_global(&self) -> String {
        if self.symbol_prefix.is_empty() {
            crate::block::DEFAULT_NULL_GUARD_GLOBAL.to_string()
        } else {
            format!(
                "{}_{}",
                crate::block::DEFAULT_NULL_GUARD_GLOBAL,
                self.symbol_prefix
            )
        }
    }

    /// Register the module's `preserve_nonecc` symbols (#8175). Must be
    /// called before any call site to one of them is emitted — in practice,
    /// right after the specialization plan is selected and before any user
    /// function body compiles. The shared cell means functions defined
    /// earlier (init preludes, string pools) see the same registry.
    pub(crate) fn set_preserve_none_fns(&mut self, fns: impl IntoIterator<Item = String>) {
        self.preserve_none_fns.borrow_mut().extend(fns);
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
    /// Invoke-EH (#7302): declare the personality routine referenced by
    /// every `define ... personality ptr @perry_eh_personality`. Declared
    /// varargs — the symbol is only ever *named* on define lines and in the
    /// unwind tables; generated code never calls it.
    pub fn declare_personality(&mut self) {
        for name in ["perry_eh_personality", "perry_iterator_eh_personality"] {
            if self.declared_names.insert(name.to_string()) {
                self.declarations
                    .push((name.to_string(), format!("declare i32 @{name}(...)")));
            }
        }
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
        let name = name.into();
        self.defined_names.insert(name.clone());
        let func = LlFunction::new_with_fp_flags(name, return_type, params, self.fp_flags);
        // #8175: every function shares the module's preserve_nonecc registry,
        // so its call sites and its own define header agree on the convention.
        func.set_preserve_none_fns(Rc::clone(&self.preserve_none_fns));
        func.set_fn_infos(Rc::clone(&self.fn_infos));
        func.set_null_guard_global(&self.null_guard_global());
        self.functions.push(func);
        self.functions.last_mut().unwrap()
    }

    /// Record a fact about `body`, a JS body this module defines, in its
    /// `JsFunctionInfo` (`crate::fn_info`).
    pub(crate) fn note_fn_info(
        &self,
        body: &str,
        note: impl FnOnce(&mut crate::fn_info::FnInfoFacts),
    ) {
        note(self.fn_infos.borrow_mut().facts_mut(body));
    }

    /// Attach retained source directly to an ordinary compiled JS body's
    /// static info. Raw method/accessor ABIs deliberately decline: their
    /// reflected function objects run shared runtime thunks and still use the
    /// copying/borrowing registry path in `string_pool`.
    pub(crate) fn attach_fn_source(
        &self,
        body: &str,
        global: &str,
        offset: usize,
        byte_len: usize,
        is_non_strict_ordinary: bool,
    ) -> bool {
        let Some(function) = self.function_named(body) else {
            return false;
        };
        if !is_js_body(function) {
            return false;
        }
        self.fn_infos
            .borrow_mut()
            .facts_mut(body)
            .set_source(crate::fn_info::RetainedSource {
                global: global.to_string(),
                offset,
                byte_len,
                is_non_strict_ordinary,
            });
        true
    }

    pub(crate) fn request_static_seed_body(&mut self, body: &str) {
        self.fn_infos.borrow_mut().request_static_seed_body(body);
    }

    /// Emit the module's `JsFunctionInfo` globals (`crate::fn_info`): one
    /// definition per body this module defines that is allocated here, has
    /// recorded facts, or is an external-linkage value wrapper another module
    /// may allocate; an `external` declaration for every allocated body
    /// another module defines. Runs once, after every function exists.
    pub(crate) fn emit_fn_infos(&mut self, permanent_image: bool) {
        let lines = {
            let functions = &self.functions;
            let by_name: std::collections::HashMap<&str, &LlFunction> =
                functions.iter().map(|f| (f.name.as_str(), f)).collect();
            let exported = functions
                .iter()
                .filter(|f| {
                    f.linkage.is_empty() && f.name.starts_with("__perry_wrap_") && is_js_body(f)
                })
                .map(|f| f.name.clone());
            self.fn_infos.borrow().render_globals(
                |body| {
                    let f = by_name.get(body)?;
                    Some(crate::fn_info::DefinedBody {
                        params: f.params.len().saturating_sub(2),
                        linkage: f.linkage.clone(),
                    })
                },
                exported,
                permanent_image,
            )
        };
        self.globals.extend(lines);
    }

    /// A defined function by symbol name.
    pub(crate) fn function_named(&self, name: &str) -> Option<&LlFunction> {
        self.functions.iter().find(|f| f.name == name)
    }

    pub fn function_mut(&mut self, idx: usize) -> Option<&mut LlFunction> {
        self.functions.get_mut(idx)
    }

    /// Render-free body-size estimate for an already-lowered function.
    ///
    /// Guarded entry wrappers are emitted after their private specialization
    /// bodies.  They use this lookup to decide whether flattening that body
    /// before statepoint rewriting stays inside the explicit native-roots
    /// code-size budget.
    pub(crate) fn function_estimated_ir_bytes(&self, name: &str) -> Option<usize> {
        self.functions
            .iter()
            .find(|function| function.name == name)
            .map(LlFunction::estimated_ir_bytes)
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

    /// Render-free size estimate for the function bodies that LLVM will see.
    /// Used after lowering to size codegen units by actual generated IR rather
    /// than HIR callable count (a poor proxy for minified/generated programs).
    pub(crate) fn estimated_function_ir_bytes(&self) -> usize {
        self.deduped_function_refs()
            .iter()
            .map(|f| f.estimated_ir_bytes())
            .sum()
    }

    /// True if a function with the given name has already been *defined*
    /// in this module. Used by the #461 export-stub pass to avoid
    /// redefining a symbol that an earlier emission path (function body,
    /// value-getter, #460 forwarding wrapper) already claimed.
    pub fn has_function(&self, name: &str) -> bool {
        self.defined_names.contains(name)
    }

    pub fn add_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = global {} {}", name, ty, init));
    }

    /// An external thread-local global with an explicit TLS model
    /// (`initialexec`, ...): a runtime `#[thread_local]` generated code names.
    pub fn add_external_tls_global(&mut self, name: &str, ty: &str, model: &str) {
        self.globals.push(format!(
            "@{name} = external thread_local({model}) global {ty}"
        ));
    }

    pub fn add_external_global(&mut self, name: &str, ty: LlvmType) {
        self.globals
            .push(format!("@{} = external global {}", name, ty));
    }

    pub fn add_internal_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = internal global {} {}", name, ty, init));
    }

    /// #10399: the thread-local form of [`Self::add_global`]. Emitted only
    /// when the program constructs a `worker_threads` Worker
    /// (`program_has_worker`), so every thread instantiates its own copy of
    /// the module graph the way Node and bun do. A program with no worker
    /// keeps the process-wide form and pays nothing.
    ///
    /// The TLS model is not spelled here: the units are compiled
    /// position-independent, where LLVM must assume the general-dynamic model.
    /// In an executable the in-process emitter switches every such global to
    /// local-exec (`inprocess::optimize_emit::use_local_exec_tls`).
    pub fn add_thread_local_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = thread_local global {} {}", name, ty, init));
    }

    /// #10399: the thread-local form of [`Self::add_internal_global`].
    pub fn add_internal_thread_local_global(&mut self, name: &str, ty: &str, init: &str) {
        self.globals.push(format!(
            "@{} = internal thread_local global {} {}",
            name, ty, init
        ));
    }

    /// #10399: a global that **module init writes**.
    ///
    /// When the program constructs a `worker_threads` Worker every thread
    /// runs its own module init (the `__perry_init_done_*` guard is
    /// thread-local), so each of these slots must be per-thread too —
    /// otherwise a worker's init would overwrite the main thread's slot with
    /// a pointer into the worker's own arena and corrupt the main thread.
    /// The two properties travel together and must never be split.
    ///
    /// External linkage is preserved: module-global slots are deliberately
    /// non-`internal` so clang cannot constant-fold reads to 0.0 across TUs.
    pub fn add_module_state_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        if self.program_has_worker {
            self.add_thread_local_global(name, ty, init);
        } else {
            self.add_global(name, ty, init);
        }
    }

    /// `internal`-linkage sibling of [`Self::add_module_state_global`].
    pub fn add_internal_module_state_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        if self.program_has_worker {
            self.add_internal_thread_local_global(name, ty, init);
        } else {
            self.add_internal_global(name, ty, init);
        }
    }

    /// #10399: declare a module-state global that ANOTHER module defines.
    /// The TLS specifier is part of the symbol's identity, so a declaration
    /// must match the definition or the link fails with
    /// "TLS definition ... mismatches non-TLS reference".
    pub fn add_external_module_state_global(&mut self, name: &str, ty: LlvmType) {
        if self.program_has_worker {
            // Same collection as `add_external_global`. Pushing to
            // `declarations` instead put the line outside the owner/dedup
            // bookkeeping that `globals` gets, and `freeze_unit` then copied
            // it into every codegen unit on top of whatever the globals path
            // already emitted — "error: redefinition of global".
            self.globals
                .push(format!("@{} = external thread_local global {}", name, ty));
        } else {
            self.add_external_global(name, ty);
        }
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

    /// Emit the module's one cold retained-source blob outside the ordinary
    /// literal pages. This is a byte blob, not a C string: every consumer
    /// carries an explicit byte length, so no terminator is emitted.
    pub fn add_retained_source_constant(&mut self, value: &str) -> (String, usize) {
        let name = if self.symbol_prefix.is_empty() {
            ".perry.retained_source".to_string()
        } else {
            format!("{}_.perry.retained_source", self.symbol_prefix)
        };
        let bytes = value.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 8);
        lit.push_str("c\"");
        for &byte in bytes {
            if (32..127).contains(&byte) && byte != b'"' && byte != b'\\' {
                lit.push(byte as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{byte:02X}"));
            }
        }
        lit.push('"');
        let section = if self.target_triple.contains("apple") {
            "__TEXT,__perry_src"
        } else if self.target_triple.contains("windows") {
            ".rdata$perry_src"
        } else {
            ".perry_src"
        };
        self.string_constants.push(format!(
            "@{name} = private constant [{} x i8] {lit}, section \"{section}\", align 1",
            bytes.len()
        ));
        (name, bytes.len())
    }

    /// Add a UTF-8 string constant to the module's constant pool. Returns
    /// `(global_name, byte_length)` — the byte length is what Perry passes as
    /// the `len` argument to `js_string_from_bytes`.
    ///
    /// The name is `@<prefix>_.str.N` once [`Self::set_symbol_prefix`] has
    /// run (bare `@.str.N` otherwise). The constant is `private` here, but
    /// codegen-unit splitting promotes it to a link-visible symbol, so the
    /// name must already be unique across the whole program — see the
    /// `symbol_prefix` field.
    pub fn add_string_constant(&mut self, value: &str) -> (String, usize) {
        let name = if self.symbol_prefix.is_empty() {
            format!(".str.{}", self.string_counter)
        } else {
            format!("{}_.str.{}", self.symbol_prefix, self.string_counter)
        };
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
    /// definitions — header, string constants, globals, declarations,
    /// attribute groups and metadata. Locally-defined functions are emitted
    /// as declarations so globals with relative function references resolve
    /// when the skeleton is parsed on its own.
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
        ir.push_str(&format!("source_filename = \"{MODULE_SOURCE_NAME}\"\n"));
        ir.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));
        if crate::codegen::helpers::native_stack_roots_enabled()
            && self.target_triple.contains("apple")
        {
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
        for f in funcs {
            ir.push_str(&declare_line_for(f));
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
        ir.push_str(&format!("source_filename = \"{MODULE_SOURCE_NAME}\"\n"));
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
        let gc_leaf_callees = if crate::codegen::helpers::native_stack_roots_enabled() {
            crate::gc_call_effects::transitive_leaf_functions(&funcs)
        } else {
            HashSet::new()
        };

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
            ir.push_str(&func.to_ir_with_gc_leaf_callees(&gc_leaf_callees));
            ir.push('\n');
        }

        self.push_attrs_and_metadata(&mut ir);

        ir
    }

    /// Emit the shared setjmp attribute groups + the `!0`/buffer-alias metadata
    /// tail. Factored out of [`to_ir`] so each codegen unit can replicate the
    /// same attributes and metadata (so `#0`/`#1` and `!N` references resolve in
    /// every unit). Over-emitting an unused attribute group is harmless.
    fn push_attrs(&self, ir: &mut String) {
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
    }

    fn push_attrs_and_metadata(&self, ir: &mut String) {
        self.push_attrs(ir);
        // Issue #52: `!0 = !{}` referenced by `!invariant.load !0`, plus the
        // buffer alias-scope metadata. LICM/GVN hoist invariant loads out of
        // loops only with these present.
        ir.push_str("\n!0 = !{}\n");
        for ml in &self.metadata_lines {
            ir.push_str(ml);
            ir.push('\n');
        }
    }

    /// Emit only metadata nodes reachable from one codegen unit's function
    /// bodies. Buffer alias metadata is numbered module-wide; replicating its
    /// complete table into every unit gave full Claude a ~15 MiB per-unit floor
    /// and duplicated gigabytes of parse input. References between metadata
    /// nodes are closed transitively (scope lists -> scopes -> domain), while
    /// preserving original definition order for deterministic output.
    fn push_attrs_and_referenced_metadata_ids(&self, ir: &mut String, mut needed: HashSet<u32>) {
        self.push_attrs(ir);
        ir.push_str("\n!0 = !{}\n");
        needed.remove(&0);
        let mut by_id: HashMap<u32, &str> = HashMap::with_capacity(self.metadata_lines.len());
        for line in &self.metadata_lines {
            if let Some(id) = metadata_definition_id(line) {
                by_id.insert(id, line);
            }
        }
        let mut work: Vec<u32> = needed.iter().copied().collect();
        while let Some(id) = work.pop() {
            let Some(line) = by_id.get(&id) else { continue };
            let mut refs = HashSet::new();
            collect_metadata_refs(line, &mut refs);
            for referenced in refs {
                if referenced != id && referenced != 0 && needed.insert(referenced) {
                    work.push(referenced);
                }
            }
        }
        for line in &self.metadata_lines {
            if metadata_definition_id(line).is_some_and(|id| needed.contains(&id)) {
                ir.push_str(line);
                ir.push('\n');
            }
        }
    }

    /// Render this module as `n` independent codegen-unit `.ll` texts (#5391).
    ///
    /// Each unit is independently compilable by `clang -c`, so peak compiler
    /// memory is bounded to ~1/n of the whole module — the structural fix for
    /// the single giant translation unit that makes clang OOM on large bundles.
    ///
    /// The functions are split into `n` contiguous buckets. Every unit carries:
    ///   * the string constants + globals it references, with local-linkage
    ///     and bare external DEFINITIONS promoted to `linkonce_odr` when more
    ///     than one unit defines them (the linker keeps one copy) and left in
    ///     their original linkage otherwise (#9610). Globals are a tiny
    ///     fraction of a large module's IR, so the duplication is cheap;
    ///     `external` *declarations* are replicated as-is;
    ///   * the module's external `declare`s plus a synthesized `declare` for
    ///     every locally-defined function the unit does NOT itself define, so
    ///     cross-unit calls resolve at link time (deduped by name, local
    ///     definitions supply the authoritative signature);
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
            let gc_leaf_callees =
                Arc::new(if crate::codegen::helpers::native_stack_roots_enabled() {
                    crate::gc_call_effects::transitive_leaf_functions(&funcs)
                } else {
                    HashSet::new()
                });
            return vec![CodegenUnitPart {
                pre: String::new(),
                post: String::new(),
                funcs,
                gc_leaf_callees,
            }];
        }
        self.layout_codegen_units(
            funcs,
            n,
            crate::workers::unit_workers(),
            |f, _text, _bytes| f,
        )
    }

    /// Consuming twin of [`Self::codegen_unit_parts`] for the native LLVM API
    /// path, for `n > 1` and more than one function. The layout is the same;
    /// each function moves out of the module and is handed to `freeze`, in
    /// unit order, together with its final text and estimated IR size, the
    /// moment the layout's reference scan has rendered it. That one rendering
    /// is all the native path needs from a function: `freeze` turns it into
    /// the worker payload and the function's lowering-owned graph is released
    /// right there, so the module is rendered once rather than once for the
    /// scan and again per unit, and the payload replaces the graph instead of
    /// accumulating beside it.
    pub(crate) fn into_codegen_unit_parts_with<T>(
        mut self,
        n: usize,
        workers: usize,
        freeze: impl FnMut(LlFunction, &str, usize) -> T,
    ) -> Vec<OwnedCodegenUnitPart<T>> {
        let mut seen: HashSet<String> = HashSet::with_capacity(self.functions.len());
        let funcs: Vec<LlFunction> = std::mem::take(&mut self.functions)
            .into_iter()
            .filter(|function| seen.insert(function.name.clone()))
            .collect();
        self.layout_codegen_units(funcs, n, workers, freeze)
    }

    /// The unit layout both unit backends share (`codegen_unit_parts`,
    /// `into_codegen_unit_parts_with`): bucket the deduplicated `funcs`, scan
    /// each function's final text for the globals, declarations and metadata
    /// its unit needs, and assemble each unit's `pre`/`post`. `visit` receives
    /// every function once, in unit order, with the text the scan read.
    /// `workers` bounds the threads that finish the rendered texts.
    ///
    /// The scan reads `to_ir_with_gc_leaf_callees` — the exact text a unit
    /// compiles. It names the same symbols and metadata as the forced-external
    /// rendering: forcing external linkage only drops a `define` linkage word,
    /// and the leaf annotation only appends a `"gc-leaf-function"` attribute.
    fn layout_codegen_units<F, T>(
        &self,
        funcs: Vec<F>,
        n: usize,
        workers: usize,
        mut visit: impl FnMut(F, &str, usize) -> T,
    ) -> Vec<OwnedCodegenUnitPart<T>>
    where
        F: std::borrow::Borrow<LlFunction>,
    {
        let gc_leaf_callees = {
            let refs: Vec<&LlFunction> = funcs.iter().map(|f| f.borrow()).collect();
            Arc::new(if crate::codegen::helpers::native_stack_roots_enabled() {
                crate::gc_call_effects::transitive_leaf_functions(&refs)
            } else {
                HashSet::new()
            })
        };
        let n = n.min(funcs.len());
        // Balance units by estimated byte size, not function count: minified
        // bundles have a few enormous functions (a 68MB IIFE in the cli.js
        // case), so contiguous count-chunking can clump them into one outsized
        // unit whose LLVM optimization time dominates. Greedy largest-first bin-packing
        // assigns each function to the currently-smallest unit, isolating big
        // functions and keeping the rest even. (A single function larger than
        // total/n is irreducible here; that requires structured outlining inside
        // codegen, not something inter-function partitioning can divide.)
        let sizes: Vec<usize> = funcs
            .iter()
            .map(|f| f.borrow().estimated_ir_bytes())
            .collect();
        let mut order: Vec<usize> = (0..funcs.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut bucket_bytes = vec![0usize; n];
        for &i in &order {
            let target = bucket_bytes
                .iter()
                .enumerate()
                .min_by_key(|&(_, &b)| b)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            buckets[target].push(i);
            bucket_bytes[target] += sizes[i];
        }
        let bucket_names: Vec<Vec<String>> = buckets
            .iter()
            .map(|bucket| {
                bucket
                    .iter()
                    .map(|&i| funcs[i].borrow().name.clone())
                    .collect()
            })
            .collect();

        // Definitions are carried in their ORIGINAL linkage here; the
        // duplicate-safe promotion below is applied per unit, and only to the
        // globals that more than one unit actually defines (#9610).
        let shared_strings: Vec<String> = self.string_constants.clone();
        let shared_globals: Vec<String> = self.globals.clone();

        // name -> declare line. Start with module declarations (runtime, FFI,
        // cross-module), then replace any entry that is also defined locally
        // with a declaration synthesized from that definition. Import metadata
        // can contain an earlier, less precise signature; the definition is what
        // the whole-module renderer and LLVM see, so split units must agree with
        // it too. Deduped by name so no unit emits a duplicate declaration.
        // BTreeMap keeps unit output deterministic.
        let mut decl_by_name: BTreeMap<String, String> = BTreeMap::new();
        for (name, decl) in &self.declarations {
            decl_by_name.insert(name.clone(), decl.clone());
        }
        for f in &funcs {
            let f = f.borrow();
            decl_by_name.insert(f.name.clone(), declare_line_for(f));
        }
        // #10399: the comment above promises this for anything "defined
        // locally", but only functions got it. A GLOBAL this module defines
        // can also sit in `self.declarations` as an `external` line (import
        // metadata declares a class-keys / ShapeId / module-value slot before
        // the defining pass runs). The stale entry then wins in every unit
        // that does not define the global.
        //
        // That was harmless while every global was non-TLS. It is not
        // harmless now: when the definition is `thread_local` and the stale
        // declaration is not, `ld -r` rejects the module with
        //   "TLS definition in <unit>.o section .tbss mismatches
        //    non-TLS reference in <other unit>.o"
        // which is how three prettier plugins stopped compiling. Synthesize
        // the declaration from the definition, exactly as the function arm
        // does, so the two always agree.
        for def in &shared_globals {
            if def.contains(" = external ") {
                continue;
            }
            let Some(sym) = global_symbol_name(def) else {
                continue;
            };
            let name = sym.trim_start_matches('@');
            let Some(decl) = external_decl_for_global(def) else {
                continue;
            };
            if let Some(slot) = decl_by_name.get_mut(name) {
                *slot = decl;
            }
        }

        // #7174 (real-app scaling): scan each bucket's functions first, then
        // give every global/string exactly ONE defining unit and hand the rest
        // an `external` declaration. Replicating all definitions into every
        // unit made per-unit IR grow with unit COUNT — on the 13 MB Claude Code
        // bundle that meant ~400 MB units and `clang: translation unit is too
        // large ... ran out of source locations`, no matter how finely it was
        // split. Definitions are already `linkonce_odr` (visible), so an
        // external declaration resolves to the same symbol at link time.
        // Scan one function at a time and discard its text immediately. The
        // native API path needs only these reference sets, not a retained
        // module-scale `.ll` duplicate beside the lowering-owned IR graph.
        let mut bucket_refs: Vec<HashSet<String>> =
            (0..buckets.len()).map(|_| HashSet::new()).collect();
        let mut bucket_metadata_refs: Vec<HashSet<u32>> =
            (0..buckets.len()).map(|_| HashSet::new()).collect();
        //
        // Each unit's functions are rendered on this thread (the function
        // graph is not shareable), their whole-function text passes run on
        // the unit workers (`TextFinish` is a function of the text alone),
        // and the finished texts are scanned and handed to `visit` in unit
        // order — so the result does not depend on the worker count.
        let mut slots: Vec<Option<F>> = funcs.into_iter().map(Some).collect();
        let mut bucket_items: Vec<Vec<T>> = Vec::with_capacity(buckets.len());
        for (bi, bucket) in buckets.iter().enumerate() {
            let mut owned: Vec<F> = Vec::with_capacity(bucket.len());
            let mut pending: Vec<(String, crate::function::TextFinish)> =
                Vec::with_capacity(bucket.len());
            for &i in bucket {
                let func = slots[i].take().expect("each function belongs to one unit");
                let function = func.borrow();
                pending.push((
                    function.render_unfinished(),
                    function.text_finish(&gc_leaf_callees),
                ));
                owned.push(func);
            }
            let texts = finish_texts(pending, &gc_leaf_callees, workers);
            let mut items = Vec::with_capacity(bucket.len());
            for ((func, text), &i) in owned.into_iter().zip(texts).zip(bucket) {
                collect_symbol_refs(&text, &mut bucket_refs[bi]);
                collect_metadata_refs(&text, &mut bucket_metadata_refs[bi]);
                items.push(visit(func, &text, sizes[i]));
            }
            bucket_items.push(items);
        }

        // A global is emitted into every unit that REFERENCES it — normally
        // exactly one, and `linkonce_odr` lets the linker fold the rare
        // multi-unit case (only that case: see `defining_unit_count` below).
        // Definition-in-one-unit + `external` elsewhere was tried first and is
        // subtly wrong under `-dead_strip`: the sole definition can be
        // discarded with its unit's atoms while a live reference survives in
        // another object.
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
        let mut bucket_needs: Vec<HashSet<usize>> = bucket_refs
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
        // COFF cannot safely fold every generated COMDAT here: globals whose
        // initializers name unit-local functions can acquire conflicting weak
        // associative targets (LNK1227). Give each global one owner and use
        // external declarations in other consumers. Mach-O retains the
        // replicated policy required by `-dead_strip`.
        let global_owners: Vec<usize> = (0..all_globals.len())
            .map(|gi| {
                bucket_needs
                    .iter()
                    .position(|need| need.contains(&gi))
                    .unwrap_or(0)
            })
            .collect();
        // #10152: otherwise-unreferenced globals are retained in unit 0, but
        // their initializers were absent from the function-rooted closure
        // above. An orphaned string dispatch descriptor can still name bytes
        // owned by another unit. Close those retained roots too, AFTER choosing
        // owners so ELF/COFF keep existing definitions and only add declares;
        // Mach-O's replication counts below include the added dependencies.
        let mut work: Vec<usize> = global_owners
            .iter()
            .enumerate()
            .filter_map(|(gi, &owner)| (owner == 0 && bucket_needs[0].insert(gi)).then_some(gi))
            .collect();
        while let Some(gi) = work.pop() {
            for nm in &global_refs[gi] {
                if let Some(&next) = global_index.get(nm.as_str()) {
                    if bucket_needs[0].insert(next) {
                        work.push(next);
                    }
                }
            }
        }
        let replicate_globals = self.target_triple.contains("apple");
        // #9610: how many units end up DEFINING each global. Under the
        // replicated (Mach-O) policy that is one unit per referencing bucket;
        // the owner fallback keeps unreferenced globals at one. Only the
        // globals a link would see twice need `linkonce_odr` to fold, and
        // linkage is not free: LLVM's Mach-O section picker sends every
        // weak-for-linker global to the coalesced *data* section, so a
        // `zeroinitializer` global promoted for no reason leaves
        // `__DATA,__bss` (zerofill, no file bytes) for file-backed
        // `__DATA,__data`. Per-site inline caches are `[12 x i64]
        // zeroinitializer` referenced by exactly one function each — 25.16 MB
        // of literal zeros in the Claude Code binary's `__data`, 8.2% of the
        // file, purely from the promotion. Only LOCAL-linkage definitions skip
        // it (`has_local_linkage`) — that covers every generated cache and
        // table. An EXTERNAL definition keeps its cross-module coalescing, as
        // `weak_odr` rather than `linkonce_odr`: both fold same-named copies,
        // but `linkonce_odr` is discardable, and LLVM drops a sole definition
        // that nothing in its own unit uses even when another object of the
        // link names it (a ConstFn body's `$info`, referenced only by the
        // static shape-seed object, vanished from the binary that way).
        let mut defining_unit_count: Vec<usize> = vec![0; all_globals.len()];
        if replicate_globals {
            for need in &bucket_needs {
                for &gi in need {
                    defining_unit_count[gi] += 1;
                }
            }
        }
        for count in &mut defining_unit_count {
            *count = (*count).max(1);
        }

        let unit_posts: Vec<String> = bucket_metadata_refs
            .into_iter()
            .map(|metadata_refs| {
                let mut post = String::new();
                self.push_attrs_and_referenced_metadata_ids(&mut post, metadata_refs);
                post
            })
            .collect();

        let mut parts = Vec::with_capacity(n);
        for (bi, items) in bucket_items.into_iter().enumerate() {
            let defined: HashSet<&str> = bucket_names[bi].iter().map(String::as_str).collect();
            let mut pre = String::new();
            pre.push_str("; Generated by perry-codegen (codegen unit)\n");
            pre.push_str(&format!("source_filename = \"{MODULE_SOURCE_NAME}\"\n"));
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
                let owns = global_owners[gi] == bi;
                if (replicate_globals && referenced) || owns {
                    if replicate_globals {
                        if !has_local_linkage(def) {
                            // External: another object may name it, so it must
                            // coalesce without ever being discarded.
                            pre.push_str(&promote_external_global_for_units(def));
                        } else if defining_unit_count[gi] > 1 {
                            pre.push_str(&promote_global_for_units(def));
                        } else {
                            pre.push_str(def);
                        }
                    } else {
                        pre.push_str(&make_unique_owner_global(def));
                    }
                    pre.push('\n');
                } else if referenced {
                    let decl = external_decl_for_global(def).unwrap_or_else(|| {
                        panic!("cannot form external declaration for generated global: {def}")
                    });
                    pre.push_str(&decl);
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
            for (gi, refs) in global_refs.iter().enumerate() {
                let referenced = bucket_needs[bi].contains(&gi);
                let owns = global_owners[gi] == bi;
                // Include references from every global whose initializer is
                // actually emitted in this unit. Unit 0 owns otherwise-dead
                // anchor globals, including static ClosureHeaders that name
                // an `__perry_wrap_extern_*` function. Those globals are not
                // in `bucket_needs` (no function references them), but their
                // initializer still requires a cross-unit function declare.
                // Merely external declarations in non-owning COFF units have
                // no initializer, so they contribute no symbol references.
                let emits_definition = (replicate_globals && referenced) || owns;
                if !emits_definition {
                    continue;
                }
                for nm in refs {
                    needed.insert(nm.trim_start_matches('@'));
                }
            }
            for (name, decl) in &decl_by_name {
                if defined.contains(name.as_str()) || !needed.contains(name.as_str()) {
                    continue;
                }
                pre.push_str(decl);
                pre.push('\n');
            }
            if crate::codegen::helpers::native_stack_roots_enabled() {
                push_statepoint_declarations(&mut pre);
            }
            pre.push('\n');

            parts.push(OwnedCodegenUnitPart {
                pre,
                post: unit_posts[bi].clone(),
                funcs: items,
                gc_leaf_callees: Arc::clone(&gc_leaf_callees),
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
                    ir.push_str(&render_fn_external_with_gc_leaf_callees(
                        func,
                        &part.gc_leaf_callees,
                    ));
                    ir.push('\n');
                }
                ir.push_str(&part.post);
                ir
            })
            .collect()
    }
}

/// Apply each function's [`crate::function::TextFinish`] to its rendered text
/// on up to `workers` threads. The result is in input order whatever the
/// worker count, and identical to finishing each text on the calling thread.
fn finish_texts(
    pending: Vec<(String, crate::function::TextFinish)>,
    gc_leaf_callees: &HashSet<String>,
    workers: usize,
) -> Vec<String> {
    let workers = workers.min(pending.len());
    if workers <= 1 {
        return pending
            .into_iter()
            .map(|(text, finish)| finish.apply(text, gc_leaf_callees))
            .collect();
    }
    let total = pending.len();
    let queue: Vec<std::sync::Mutex<Option<(String, crate::function::TextFinish)>>> = pending
        .into_iter()
        .map(|item| std::sync::Mutex::new(Some(item)))
        .collect();
    let done: Vec<std::sync::Mutex<Option<String>>> =
        (0..total).map(|_| std::sync::Mutex::new(None)).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let (queue, done, next) = (&queue, &done, &next);
            // The precise-root lowering walks a whole function; a bundle's
            // largest bodies are millions of instructions. Same deep stack as
            // the LLVM unit workers.
            std::thread::Builder::new()
                .name(format!("perry-text-finish-{worker}"))
                .stack_size(64 * 1024 * 1024)
                .spawn_scoped(scope, move || loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= total {
                        break;
                    }
                    let (text, finish) = queue[i]
                        .lock()
                        .expect("text finish queue poisoned")
                        .take()
                        .expect("each text is finished once");
                    let finished = finish.apply(text, gc_leaf_callees);
                    *done[i].lock().expect("text finish results poisoned") = Some(finished);
                })
                .expect("spawn text finish worker");
        }
    });
    done.into_iter()
        .map(|slot| {
            slot.into_inner()
                .expect("text finish results poisoned")
                .expect("every text is finished")
        })
        .collect()
}

/// One codegen unit, pre-render: the textual skeleton around the functions
/// (`pre` = header/strings/globals/cross-unit declares; `post` = shared
/// attribute groups + metadata) plus the functions themselves, un-rendered so
/// the native backend can construct them directly.
pub(crate) type CodegenUnitPart<'m> = OwnedCodegenUnitPart<&'m LlFunction>;

/// One codegen unit as the layout produced it, with whatever the caller kept
/// of each of its functions (a borrow, or the native worker payload).
pub(crate) struct OwnedCodegenUnitPart<T> {
    pub pre: String,
    pub post: String,
    pub funcs: Vec<T>,
    pub gc_leaf_callees: Arc<HashSet<String>>,
}

#[cfg(test)]
mod tests;

/// Whether `f` is a JS body (`double body(i64 %this_closure, i64 %js_this, ...)`,
/// `perry_abi::JS_BODY_*`).
fn is_js_body(f: &LlFunction) -> bool {
    f.params.len() >= 2
        && f.params[0].1 == crate::expr::body_call::JS_BODY_CALLEE
        && f.params[1].1 == crate::expr::body_call::JS_BODY_THIS
}
