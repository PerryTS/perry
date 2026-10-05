//! String pool emission. Split out of `codegen.rs` (now `codegen/mod.rs`).

use std::collections::HashMap;

use crate::block::LlBlock;
use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{DOUBLE, I32, I64, I8, PTR, VOID};

use super::ctor_arity::constructor_layout_params;
use super::helpers::{sanitize_member, scoped_static_method_name};
use super::retained_source_pool::{SourcePool, SourceRange};
use super::spec_function_length;

/// Emits a long sequence of INDEPENDENT init operations (string allocation,
/// closure/class/function registration) into a series of small chunk functions
/// instead of one giant function, then exposes the chunk names so the entry
/// `__perry_init_strings_*` can call them in order (#5391 function splitting).
///
/// A large bundle interns ~190K strings and registers tens of thousands of
/// closures/classes; emitting all of it into ONE function produced a single
/// ~32MB / ~400K-instruction basic block, which is catastrophically expensive
/// for LLVM to optimize as one function (~36 min). Every op here is independent
/// — each writes to its own global or a
/// runtime registry; no SSA value flows between ops — so splitting at op
/// boundaries is safe and order-preserving (chunks run in sequence, ops in order
/// within a chunk).
#[derive(Default)]
struct InitChunks {
    ops: usize,
    current: Option<usize>,
    names: Vec<String>,
}

struct InitChunker<'a> {
    llmod: &'a mut LlModule,
    base_name: String,
    ops_per_chunk: usize,
    // Literal infrastructure may run before cyclic dependencies. Declared
    // class metadata retains its existing module-body initialization boundary.
    literals: bool,
    phases: [InitChunks; 2],
}

impl<'a> InitChunker<'a> {
    fn new(llmod: &'a mut LlModule, base_name: String, ops_per_chunk: usize) -> Self {
        Self {
            llmod,
            base_name,
            ops_per_chunk: ops_per_chunk.max(1),
            literals: true,
            phases: Default::default(),
        }
    }

    fn module(&mut self) -> &mut LlModule {
        self.llmod
    }

    fn roll_if_full(&mut self) {
        let phase = usize::from(!self.literals);
        let state = &mut self.phases[phase];
        if state.current.is_none() || state.ops >= self.ops_per_chunk {
            if let Some(current) = state.current {
                self.llmod
                    .function_mut(current)
                    .unwrap()
                    .block_mut(0)
                    .unwrap()
                    .ret_void();
            }
            let name = format!(
                "{}_{}_chunk{}",
                self.base_name,
                if self.literals { "literal" } else { "class" },
                state.names.len()
            );
            self.llmod
                .define_function(&name, VOID, vec![])
                .create_block("entry");
            state.current = Some(self.llmod.function_count() - 1);
            state.names.push(name);
            state.ops = 0;
        }
    }

    fn current_block(&mut self) -> &mut LlBlock {
        let state = &mut self.phases[usize::from(!self.literals)];
        state.ops += 1;
        self.llmod
            .function_mut(state.current.unwrap())
            .unwrap()
            .block_mut(0)
            .unwrap()
    }

    fn finish(self) -> [Vec<String>; 2] {
        self.phases.map(|state| {
            if let Some(current) = state.current {
                self.llmod
                    .function_mut(current)
                    .unwrap()
                    .block_mut(0)
                    .unwrap()
                    .ret_void();
            }
            state.names
        })
    }
}

/// Pool literals at most this long are minted as ATOMS. **Must equal
/// `INTERN_MAX_BYTE_LEN` in `perry-runtime/src/string/intern.rs`**, the
/// longest key the runtime interns; `js_string_pool_atom` falls back to a
/// plain allocation past it, so a mismatch costs only the atom, never
/// correctness.
pub(crate) const POOL_ATOM_MAX_BYTE_LEN: usize = 64;

/// Emit the string pool into the module: byte-array constants, handle
/// globals, and the `__perry_init_strings_<prefix>` function that
/// allocates + NaN-boxes + GC-roots each handle exactly once at startup.
///
/// The string pool was constructed with a `module_prefix`, so every
/// `entry.bytes_global` / `entry.handle_global` is already prefixed.
/// Emission uses those names directly — no extra prefixing here.
pub(super) fn emit_string_pool(
    llmod: &mut LlModule,
    strings: &StringPool,
    module_prefix: &str,
    agent_strings_tls: bool,
    // #9188 follow-up: which registration spelling the name/source loops below
    // may use. `_static` hands the registry the `@.str.N` constant itself
    // instead of a slice to copy, which is sound only while this image stays
    // mapped — true for an executable, NOT for a `dylib` plugin that
    // `perry_plugin_unload` will `dlclose`. See `runtime_decls`.
    output_type: &str,
    class_keys_init_data: &[(String, String, u32, Vec<u64>, Vec<u64>)],
    class_header_image_inits: &std::collections::HashMap<String, (u32, u64, u32)>,
    class_birth_reps: &HashMap<String, u64>,
    class_ids: &HashMap<String, u32>,
    classes: &HashMap<String, &perry_hir::Class>,
    // The classes this module defines, by identity: the registration loops
    // below (names, methods, static methods and their function-object
    // entries, constructors, accessors) key each class by its ClassId, never
    // by a name (`classes` above maps names, and two classes may share one).
    module_classes: &[perry_hir::Class],
    // #5592: user-visible `.name` overrides keyed by ClassId, for classes
    // whose HIR registration key was uniquified away from their JS name.
    class_display_names: &HashMap<u32, String>,
    // #9413: retained class SOURCE text keyed by ClassId, so `String(C)` /
    // `C.toString()` return the class source instead of a synthesized
    // `function C() { [native code] }`.
    class_source_text: &HashMap<u32, String>,
    // Wall 51: per-class standalone-constructor arity, accounting for the
    // synthesized `super(...args)` forwarding ctor a no-own-ctor class with
    // heritage inherits (its arity comes from the nearest ancestor ctor, which
    // may be cross-module). Keyed by canonical class name. Overrides the naive
    // `class.constructor.params.len()` (which is 0 for a no-own-ctor class) so
    // the registered `total_params` matches the emitted ctor's real signature.
    ctor_arity_overrides: &HashMap<String, u32>,
    closure_rest_params: &HashMap<u32, usize>,
    // Declared ABI arity for non-rest closures, used for runtime padding.
    closure_arities: &HashMap<u32, u32>,
    // ECMAScript-visible `.length` for all closures.
    closure_lengths: &HashMap<u32, u32>,
    // Closure body func_ids that came from arrow function syntax.
    closure_arrow_functions: &std::collections::HashSet<u32>,
    // Small direct arrow callbacks with an internal body that trusts their
    // compiler-installed box-capture slots after exact-target resolution.
    trusted_box_closures: &std::collections::HashMap<
        u32,
        super::closure_collect::TrustedBoxClosure,
    >,
    versioned_loop_callbacks: &std::collections::HashSet<u32>,
    // Issue #653: wrappers (`__perry_wrap_<name>`) for top-level user functions
    // that declare a rest param. Each entry is `(wrapper_symbol, fixed_arity)`
    // — a fact of the WRAPPER's `JsFunctionInfo`, NOT the underlying user
    // function's, because the wrapper is what a function value runs. Without
    // it, calling a user
    // function as a value through `js_closure_call_apply_with_spread` fed the
    // raw spread elements into the wrapper's flat `(this, a0, a1)` signature
    // instead of bundling args[fixed_arity..] into a real array — the rest
    // param then read a single element's bits as if it were the rest array.
    user_fn_wrapper_rest: &[(String, usize)],
    // Refs #915 (gap 1 from #899): subset of `closure_rest_params` whose
    // rest param is the HIR-synthesized `arguments` array. Their infos say so,
    // so the runtime bundles
    // ALL passed args (not just the trailing tail) into the rest slot —
    // matching JS spec semantics for `arguments.length`.
    closure_synthetic_arguments: &std::collections::HashSet<u32>,
    // Mirror of `closure_synthetic_arguments` for the top-level user-fn
    // wrapper path: each entry is `wrapper_symbol` whose underlying
    // function has its synthesized `arguments` rest param.
    user_fn_wrapper_synthetic_arguments: &std::collections::HashSet<String>,
    // Functions with both user `...rest` and a hidden raw-arguments slot need
    // two arrays at dynamic dispatch: the user rest tail and all arguments.
    closure_rest_and_arguments: &std::collections::HashSet<u32>,
    user_fn_wrapper_rest_and_arguments: &std::collections::HashSet<String>,
    // ABI param count for every top-level user-function wrapper
    // (`__perry_wrap_<original_name>`): the declared arity in the wrapper's
    // `JsFunctionInfo` (`.length`'s fallback). Entries for wrappers also
    // present in `user_fn_wrapper_rest` are skipped (their rest fact rules).
    user_fn_wrapper_arity: &[(String, u32)],
    // ECMAScript-visible `.length` for every top-level user-function wrapper.
    user_fn_wrapper_length: &[(String, u32)],
    // Wrapper symbols for top-level async functions. Async in their infos so
    // `util.types.isAsyncFunction` keeps working when the value is
    // observed through a runtime alias instead of direct HIR.
    user_fn_wrapper_async: &std::collections::HashSet<String>,
    // Wrapper/closure symbols whose original source form was a generator
    // function. Marked in their infos so util.types.isGeneratorFunction can distinguish
    // lowered generator state-machine closures from ordinary functions.
    user_fn_wrapper_generator: &std::collections::HashSet<String>,
    // #3664: wrapper/closure symbols whose source form was `async function*`.
    // Marked in their infos so the
    // `%AsyncGeneratorFunction%`/`%AsyncGenerator%` intrinsic chain (and
    // `util.types.isAsyncFunction`) resolve correctly for them.
    user_fn_wrapper_async_generator: &std::collections::HashSet<String>,
    // Strict-mode user functions (wrapper or inline-closure symbols).
    // Strict in their infos so call/apply/bind can apply spec
    // OrdinaryCallBindThis (#4850).
    user_fn_wrapper_strict: &std::collections::HashSet<String>,
    // `(wrapper_symbol, display_name)` for every top-level user function
    // we want `console.log` / `util.inspect` to label with the original
    // JS name. Each entry produces one `js_register_function_name_static` call
    // in `__perry_init_strings_<prefix>` so the registry is populated
    // before user code runs. See #1202.
    user_fn_display_names: &[(String, String)],
    // #4101: `(wrapper_symbol, source_text)` for every user function whose
    // original source we retained. Each entry produces one
    // `js_register_function_source_static` call in `__perry_init_strings_<prefix>`
    // so `fn.toString()` can reconstruct the source.
    user_fn_source: &[(String, String, bool)],
    // The templates the module evaluates per evaluation (`ClassExprFresh`):
    // their static methods' entries run in the function object's home.
    fresh_class_templates: &std::collections::HashSet<String>,
) {
    for entry in strings.iter() {
        // .rodata bytes — `[N+1 x i8]` because we include the null terminator.
        llmod.add_named_string_constant(&entry.bytes_global, entry.byte_len + 1, &entry.escaped_ir);
        if entry.dispatch_used {
            // AOT-stable property/method dispatch id. Unlike `handle_global`,
            // this descriptor never points into a thread-local GC arena, so
            // compiled worker closures can resolve static names without
            // sharing a main-thread StringHeader. Carrying the precomputed
            // content hash lets the runtime's per-thread materialization cache
            // avoid re-hashing the name on every property access.
            llmod.add_raw_global(format!(
                "@{} = private unnamed_addr constant {{ i32, i32, i64, ptr }} \
                 {{ i32 {}, i32 {}, i64 {}, ptr @{} }}",
                entry.dispatch_global,
                entry.byte_len,
                i32::from(entry.is_wtf8),
                crate::nanbox::i64_literal(entry.dispatch_hash),
                entry.bytes_global
            ));
        }
        // Worker module init and perry/thread's explicit string bootstrap each
        // populate this slot in the allocating agent's own arena.
        if agent_strings_tls {
            llmod.add_internal_thread_local_global(&entry.handle_global, DOUBLE, "0.0");
        } else {
            llmod.add_internal_global(&entry.handle_global, DOUBLE, "0.0");
        }
    }

    // Per-class packed-keys constants (rodata) — referenced by the
    // js_build_class_keys_array call below at module init.
    // Naming: `@perry_class_keys_packed_<modprefix>__<idx>` so we
    // don't collide with anything else.
    let mut packed_global_names: Vec<String> = Vec::with_capacity(class_keys_init_data.len());
    for (idx, (_global_name, packed, _fc, _raw_mask_words, _pointer_mask_words)) in
        class_keys_init_data.iter().enumerate()
    {
        if packed.is_empty() {
            packed_global_names.push(String::new());
            continue;
        }
        let bytes = packed.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 8);
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
        let name = format!("perry_class_keys_packed_{}__{}", module_prefix, idx);
        llmod.add_named_string_constant(&name, bytes.len() + 1, &lit);
        packed_global_names.push(name);
    }

    // Pre-allocate string constants for function-name registration. Same
    // borrow-ordering constraint as the class-name constants below: we
    // must mint the rodata globals BEFORE `init_fn` claims `&mut llmod`.
    // Each entry becomes one relative record in the module's batched function
    // name descriptor table. See #1202 and #11927.
    let mut user_fn_name_constants: Vec<(String, String, usize)> = Vec::new();
    // Deduplicated by CONTENT (#9486): the same display name is now registered
    // against several symbols — a top-level function's wrapper and its body,
    // a class method and its `__perry_wrap_*` twin — and `add_string_constant`
    // mints a fresh `@.str.N` per call, so without this every extra
    // registration also cost a duplicate copy of the name in rodata.
    // Deterministic: the map only reuses a global the loop already minted in
    // its (already sorted) input order, so emission order is unchanged (#7622).
    let mut name_constant_cache: std::collections::HashMap<&str, (String, usize)> =
        std::collections::HashMap::new();
    for (wrapper_sym, display_name) in user_fn_display_names {
        if wrapper_sym.is_empty() || display_name.is_empty() {
            continue;
        }
        let (const_name, byte_len) = match name_constant_cache.get(display_name.as_str()) {
            Some(hit) => hit.clone(),
            None => {
                let minted = llmod.add_string_constant(display_name);
                name_constant_cache.insert(display_name.as_str(), minted.clone());
                minted
            }
        };
        user_fn_name_constants.push((wrapper_sym.clone(), const_name, byte_len));
    }

    // Collect class sources in registration order before preparing the shared
    // source pool: a class can contain the exact bytes of a method/closure.
    let mut class_sources: Vec<(u32, &String)> = Vec::new();
    for (class_name, class) in classes.iter() {
        if *class_name != class.name || class_name.starts_with("__AnonShape_") {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
        if let Some(src) = class_source_text.get(&cid) {
            class_sources.push((cid, src));
        }
    }
    class_sources.sort_by_key(|entry| entry.0);
    class_sources.dedup_by_key(|(cid, _)| *cid);

    // An image that can be unloaded cannot leave borrowed source metadata in
    // either a registry or a function-info record. Executables are permanent;
    // dylibs/staticlibs retain the copying registration path below.
    let strings_outlive_registry = output_type != "dylib" && output_type != "staticlib";

    // #4101/#9413: mint source globals BEFORE `init_fn` borrows `llmod`.
    // Sharing changes only the backing bytes, never registration order, source
    // lengths, strictness flags, or the copying/static ownership contract.
    let source_pool = SourcePool::emit(
        llmod,
        user_fn_source
            .iter()
            .filter(|(symbol, source, _)| !symbol.is_empty() && !source.is_empty())
            .map(|(_, source, _)| source.as_str())
            .chain(class_sources.iter().map(|(_, source)| source.as_str())),
    );
    let mut user_fn_source_constants: Vec<(String, SourceRange, bool)> = Vec::new();
    for (wrapper_sym, source_text, is_non_strict_ordinary) in user_fn_source {
        if wrapper_sym.is_empty() || source_text.is_empty() {
            continue;
        }
        let source = source_pool.get(source_text);
        if strings_outlive_registry
            && llmod.attach_fn_source(
                wrapper_sym,
                &source.global,
                source.offset,
                source.byte_len,
                *is_non_strict_ordinary,
            )
        {
            continue;
        }
        user_fn_source_constants.push((wrapper_sym.clone(), source, *is_non_strict_ordinary));
    }

    // Pre-allocate string constants for class-name registration. We need
    // these BEFORE `init_fn` is created, because once `init_fn` borrows
    // `llmod` we can no longer mutate the module's constant pool. (#1021.)
    // Every class this module defines, keyed by identity (its ClassId).
    // Imported stubs are not here: the defining module registers them.
    let local_classes: Vec<(u32, &perry_hir::Class)> = module_classes
        .iter()
        .filter(|c| c.id != 0)
        .map(|c| (c.id, c))
        .collect();
    let mut named_class_name_constants: Vec<(u32, String, usize)> = Vec::new();
    {
        let mut named: Vec<(u32, String)> = Vec::new();
        for &(cid, class) in &local_classes {
            let class_name = &class.name;
            if !class_name.starts_with("__AnonShape_") {
                // #5592: prefer the recorded JS name when the registration
                // key was uniquified (e.g. a second `C = class {…}`).
                let js_name = class_display_names
                    .get(&cid)
                    .cloned()
                    .unwrap_or_else(|| class_name.clone());
                named.push((cid, js_name));
            }
        }
        named.sort_by_key(|a| a.0);
        named.dedup_by_key(|(cid, _)| *cid);
        for (cid, name) in named {
            let (const_name, byte_len) = llmod.add_string_constant(&name);
            named_class_name_constants.push((cid, const_name, byte_len));
        }
    }

    // #9413: the same pre-allocation for retained class source text — also
    // before `init_fn` borrows `llmod`.
    let class_source_constants: Vec<(u32, SourceRange)> = class_sources
        .into_iter()
        .map(|(cid, source)| (cid, source_pool.get(source)))
        .collect();
    drop(source_pool);

    // Emit per-class typed-shape raw-f64 and pointer-mask globals. Empty masks
    // emit no storage. Must run BEFORE
    // `init_fn = llmod.define_function(...)` because that call holds a
    // mutable borrow of `llmod` for the lifetime of the function/block
    // used by everything below.
    for (global_name, _packed, _field_count, raw_mask_words, pointer_mask_words) in
        class_keys_init_data.iter()
    {
        if !raw_mask_words.is_empty() {
            let mask_global =
                crate::typed_shape::raw_f64_mask_global_name_from_keys_global(global_name);
            let words = raw_mask_words
                .iter()
                .map(|word| format!("i64 {}", word))
                .collect::<Vec<_>>()
                .join(", ");
            llmod.add_raw_global(format!(
                "@{} = private unnamed_addr constant [{} x i64] [{}]",
                mask_global,
                raw_mask_words.len(),
                words
            ));
        }
        if !pointer_mask_words.is_empty() {
            let mask_global = crate::typed_shape::mask_global_name_from_keys_global(global_name);
            let words = pointer_mask_words
                .iter()
                .map(|word| format!("i64 {}", word))
                .collect::<Vec<_>>()
                .join(", ");
            llmod.add_raw_global(format!(
                "@{} = private unnamed_addr constant [{} x i64] [{}]",
                mask_global,
                pointer_mask_words.len(),
                words
            ));
        }
    }

    // #5391 function splitting: a large bundle interns ~190K strings AND
    // registers tens of thousands of closures/classes/functions; emitting all of
    // that into ONE `__perry_init_strings` function produced a single ~32MB /
    // ~400K-instruction basic block that LLVM handles superlinearly (~36 min).
    // Every init
    // op below is independent (each writes its own global or a runtime registry;
    // no SSA value flows between ops), so emit ALL of them — string allocation
    // and every registration loop — through `chunker`, which spills them into a
    // sequence of small `*_chunkN` functions. The entry function then just calls
    // the chunks in order. Combined with codegen-unit splitting, the chunks
    // bin-pack evenly across units instead of one unit carrying the monolith.
    let ops_per_chunk: usize = std::env::var("PERRY_STRING_INIT_CHUNK_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4000);
    let mut chunker = InitChunker::new(
        llmod,
        format!("__perry_agent_strings_{}", module_prefix),
        ops_per_chunk,
    );

    for entry in strings.iter() {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let bytes_ref = format!("@{}", entry.bytes_global);
        let handle_ref = format!("@{}", entry.handle_global);
        let len_str = entry.byte_len.to_string();
        // A literal short enough to be a property key becomes its text's ATOM
        // (`js_string_pool_atom`): one string object per key text for the
        // whole agent, shared by every module's pool, every canonical shape
        // key list and every intern hit — so a read site's key and the
        // receiver's shape key compare by pointer (S3b). Longer literals, and
        // WTF-8 ones (lone surrogates: never an identifier key), keep the
        // plain allocation.
        let atomize =
            !entry.is_wtf8 && entry.byte_len > 0 && entry.byte_len <= POOL_ATOM_MAX_BYTE_LEN;
        let handle = if atomize {
            let hash = crate::nanbox::i64_literal(entry.dispatch_hash);
            blk.call(
                I64,
                "js_string_pool_atom",
                &[(PTR, &bytes_ref), (I32, &len_str), (I64, &hash), (I32, "0")],
            )
        } else {
            let from_bytes_fn = if entry.is_wtf8 {
                "js_string_from_wtf8_bytes"
            } else {
                "js_string_from_bytes"
            };
            blk.call(I64, from_bytes_fn, &[(PTR, &bytes_ref), (I32, &len_str)])
        };
        let nanboxed = blk.call(DOUBLE, "js_nanbox_string", &[(I64, &handle)]);
        // Plain store, no remembered-set write barrier: the handle slot is
        // registered as a permanent global root on the very next line (always
        // scanned by every GC, minor and major), so a remembered-set entry is
        // redundant. No allocation runs between the store and the registration,
        // so the fresh string can't be collected in the gap. This is one-time
        // module init; emitting `js_write_barrier_root_nanbox` here added one
        // barrier per interned string for no GC benefit (and tripped the
        // native-region-proof `write_barriers_static` budget — the barriers
        // landed in `__perry_init_strings_*`, never the hot path).
        blk.store(DOUBLE, &nanboxed, &handle_ref);
        let addr_i64 = blk.ptrtoint(&handle_ref, I64);
        blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
    }

    let [string_chunks, no_class_chunks] = chunker.finish();
    debug_assert!(no_class_chunks.is_empty());
    let agent_strings_name = format!("__perry_prepare_agent_strings_{}", module_prefix);
    let ready = format!("__perry_agent_strings_ready_{}", module_prefix);
    if agent_strings_tls {
        llmod.add_internal_thread_local_global(&ready, I8, "0");
    } else {
        llmod.add_internal_global(&ready, I8, "0");
    }
    let prepare_strings = llmod.define_function(&agent_strings_name, VOID, vec![]);
    prepare_strings.create_block("entry");
    prepare_strings.create_block("prepare");
    prepare_strings.create_block("done");
    let prepare_label = prepare_strings.block_mut(1).unwrap().label.clone();
    let done_label = prepare_strings.block_mut(2).unwrap().label.clone();
    let blk = prepare_strings.block_mut(0).unwrap();
    let prepared = blk.load(I8, &format!("@{}", ready));
    let prepared = blk.icmp_ne(I8, &prepared, "0");
    blk.cond_br(&prepared, &done_label, &prepare_label);
    let blk = prepare_strings.block_mut(1).unwrap();
    for name in &string_chunks {
        blk.call_void(name, &[]);
    }
    blk.store(I8, "1", &format!("@{}", ready));
    blk.br(&done_label);
    prepare_strings.block_mut(2).unwrap().ret_void();

    let mut chunker = InitChunker::new(
        llmod,
        format!("__perry_init_strings_{}", module_prefix),
        ops_per_chunk,
    );

    // An image that can be UNLOADED cannot lend its rodata to a registry that
    // never drops entries. Perry compiles TypeScript to a dylib plugin as well
    // as an executable, and `perry_plugin_unload` ends in `dlclose` — after
    // which a borrowed `@.str.N` names unmapped memory, and the next
    // `fn.name` / `fn.toString()` / stack frame that resolves it reads that.
    // `staticlib` is included because its objects are linked into whatever
    // consumes them, which may itself be a plugin. Executables keep the
    // borrow, which is where all the volume is.
    let register_name_fn = if strings_outlive_registry {
        "js_register_function_name_static"
    } else {
        "js_register_function_name"
    };
    let register_class_source_fn = if strings_outlive_registry {
        "js_register_class_source_static"
    } else {
        "js_register_class_source"
    };

    // Register display names for top-level user functions so
    // `console.log(myFn)` prints `[Function: myFn]` instead of
    // `[Function (anonymous)]`. The runtime registry is keyed on the
    // wrapper's compiled address (`__perry_wrap_<name>`), which is
    // what `js_closure_alloc_singleton` stamps into ClosureHeader.
    // See #1202.
    if strings_outlive_registry {
        let name_table = super::function_metadata_descriptors::emit_name_table(
            chunker.module(),
            module_prefix,
            user_fn_name_constants
                .iter()
                .map(|(wrapper, name, len)| (wrapper.as_str(), name.as_str(), *len)),
        );
        if let Some(table) = name_table {
            chunker.roll_if_full();
            chunker.current_block().call_void(
                "js_register_function_names_static",
                &[
                    (PTR, &format!("@{}", table.global)),
                    (I32, &table.len.to_string()),
                ],
            );
        }
    } else {
        for (wrapper_sym, name_const, name_len) in &user_fn_name_constants {
            chunker.roll_if_full();
            chunker.current_block().call_void(
                register_name_fn,
                &[
                    (PTR, &format!("@{wrapper_sym}")),
                    (PTR, &format!("@{name_const}")),
                    (I32, &name_len.to_string()),
                ],
            );
        }
    }

    // #4101: register each function's retained source text against the same
    // wrapper/closure address `js_closure_alloc_singleton` stamps into the
    // ClosureHeader, so `fn.toString()` resolves the source by func_ptr.
    if strings_outlive_registry {
        let source_table = super::function_metadata_descriptors::emit_source_table(
            chunker.module(),
            module_prefix,
            user_fn_source_constants
                .iter()
                .map(|(wrapper, source, is_non_strict_ordinary)| {
                    (
                        wrapper.as_str(),
                        source.constant_pointer(),
                        source.byte_len,
                        *is_non_strict_ordinary,
                    )
                }),
        );
        if let Some(table) = source_table {
            chunker.roll_if_full();
            chunker.current_block().call_void(
                "js_register_function_sources_static",
                &[
                    (PTR, &format!("@{}", table.global)),
                    (I32, &table.len.to_string()),
                ],
            );
        }
    } else {
        for (wrapper_sym, source, is_non_strict_ordinary) in &user_fn_source_constants {
            chunker.roll_if_full();
            let blk = chunker.current_block();
            let source_ref = source.pointer(blk);
            blk.call_void(
                "js_register_function_source",
                &[
                    (PTR, &format!("@{wrapper_sym}")),
                    (PTR, &source_ref),
                    (I32, &source.byte_len.to_string()),
                    (I32, if *is_non_strict_ordinary { "1" } else { "0" }),
                ],
            );
        }
    }

    // #11420: every input to a class's birth [[Prototype]] identity
    // (`shapes::class_proto_id`: the anon-shape set and the generic-origin
    // edge) must be registered BEFORE the keys loop below mints the class's
    // ShapeId. The mint records `class_proto_id(cid)` in the descriptor, and
    // `try_birth_stamp_preinstalled_shape` compares it with
    // `object_proto_id(obj)` at every allocation. Registered after the mint,
    // the two disagreed for EVERY object literal: each allocation declined
    // the module-init ShapeId and was stamped with a second, freshly minted
    // one, so every guarded class-field read of a literal (a literal method's
    // `this.a`, `it.n` on a returned `{ n, warm }`) missed its shape check
    // and fell to the by-name lookup, ~900 instructions per read.
    // #7575: register the GENERIC class a monomorphized specialization came
    // from. `class Gen<T> {}` + `new Gen<number>()` emits a second class
    // `Gen$num` carrying its own class id, and the instance is stamped with
    // that id — but `x instanceof Gen` resolves the RHS to the GENERIC's id,
    // which is in no parent chain, so the walk answered `false` for the class
    // the user wrote. This is a distinct edge from the parent one on purpose:
    // `CLASS_REGISTRY`'s chain also resolves `super()`, static-method lookup
    // and vtable dispatch, so it must keep pointing at the real base.
    chunker.literals = false;
    let mut origin_pairs: Vec<(u32, u32)> = Vec::new();
    for (name, &cid) in class_ids.iter() {
        let Some(class) = classes.get(name) else {
            continue;
        };
        let Some(generic_name) = &class.specialized_from else {
            continue;
        };
        if let Some(&generic_cid) = class_ids.get(generic_name) {
            if generic_cid != 0 && generic_cid != cid {
                origin_pairs.push((cid, generic_cid));
            }
        }
    }
    origin_pairs.sort_unstable();
    for (cid, generic_cid) in origin_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        blk.call_void(
            "js_register_class_generic_origin",
            &[(I32, &cid.to_string()), (I32, &generic_cid.to_string())],
        );
    }

    // Mark each `__AnonShape_<hash>` class id. `({ x: 1 }).constructor ===
    // Object` duck-checks (date-fns / drizzle / lodash) resolve through it,
    // and it is what makes `class_proto_id` answer the ordinary object
    // prototype an anon shape's instances are born with.
    {
        let mut anon_shape_ids: Vec<u32> = Vec::new();
        for (class_name, class) in classes.iter() {
            if *class_name != class.name || !class_name.starts_with("__AnonShape_") {
                continue;
            }
            if let Some(cid) = class_ids.get(class_name).copied().filter(|&c| c != 0) {
                anon_shape_ids.push(cid);
            }
        }
        anon_shape_ids.sort_unstable();
        anon_shape_ids.dedup();
        chunker.literals = true;
        for cid in anon_shape_ids {
            chunker.roll_if_full();
            let blk = chunker.current_block();
            blk.call_void(
                "js_register_anon_shape_class_id",
                &[(crate::types::I32, &cid.to_string())],
            );
        }
    }

    // Build per-class keys arrays via js_build_class_keys_array,
    // store the result in the per-class keys global. Done ONCE at
    // module init; every `new ClassName()` call from then on does a
    // single global load + inline allocator call (no SHAPE_CACHE
    // lookup, no js_build_class_keys_array overhead).
    let literal_classes: std::collections::HashSet<_> = classes
        .values()
        .filter(|class| class.name.starts_with("__AnonShape_"))
        .filter_map(|class| class_ids.get(&class.name).copied())
        .collect();
    for (idx, (global_name, packed, field_count, _raw_mask_words, _pointer_mask_words)) in
        class_keys_init_data.iter().enumerate()
    {
        let birth = super::static_shape_ids::class_birth(
            module_prefix,
            &class_keys_init_data[idx],
            class_header_image_inits,
            class_birth_reps,
            class_ids,
        );
        // Only synthetic ordinary-object layouts are safe before dependency
        // bodies. User-class keys, prototypes and methods stay in the late phase.
        chunker.literals = literal_classes.contains(&birth.class_id);
        chunker.roll_if_full();
        let blk = chunker.current_block();
        // The birth's class id, typed-ness and live bound come from the ONE
        // derivation the driver's pre-pass also uses to name this birth's
        // content (`static_shape_ids::class_birth`); `requested` is that
        // content's static id — the definer's for a structural stub of the
        // definer's facts — (0 = none).
        let class_id = birth.class_id;
        let requested = super::static_shape_ids::requested_shape_id_for_keys_global(global_name)
            .unwrap_or(0)
            .to_string();
        let cid_str = class_id.to_string();
        // A literal birth names the plain prototype (`BirthProto::Literal`), so
        // its shape mints pass class id 0, exactly as the startup literal seed
        // (`js_shape_seed_plain`) does. Passing the anonymous class's own id was
        // equivalent only while that id had no vtable class: per-module class
        // ids collide, so an `__AnonShape_*` id can also be another module's
        // DECLARED class, and the mint then derived that class's prototype --
        // different facts under the one static id the driver assigned to this
        // content, which the mint refuses with an abort. The keys array still
        // carries the real id (`js_build_class_keys_array` above).
        let mint_cid_str = if birth.literal {
            "0".to_string()
        } else {
            cid_str.clone()
        };
        let fc_str = field_count.to_string();
        let packed_ref = if packed.is_empty() {
            "null".to_string()
        } else {
            format!("@{}", packed_global_names[idx])
        };
        let len_str = packed.len().to_string();
        // Charter step 5, T1: the birth rep rides every class mint
        // (`typed_shape::class_birth_rep_in`, one decision for the mint, the
        // inline allocation's birth fill and the store precheck), the shape
        // cache's mint beside the keys included: an outlined birth from that
        // entry carries it. It is part of the content, so the static id
        // already names it.
        let rep_str = class_birth_reps
            .get(global_name)
            .copied()
            .unwrap_or(0)
            .to_string();
        let arr = blk.call(
            I64,
            "js_build_class_keys_array",
            &[
                (I32, &cid_str),
                (I32, &fc_str),
                (PTR, &packed_ref),
                (I32, &len_str),
                (I64, &rep_str),
            ],
        );
        let global_ref = format!("@{}", global_name);
        crate::expr::emit_root_heap_word_store_on_block(blk, &arr, &global_ref);
        // #5042: register the per-class keys global as a GC root so the
        // evacuation rewrite pass fixes up its raw pointer after the keys
        // array is moved. The array lives in the longlived (old-gen) arena
        // and is held alive by the shape-cache scanner, so old-page defrag
        // (C4b) can relocate it; without registering this slot the codegen
        // global keeps a stale pointer and every `new ClassName()` afterwards
        // builds an instance over a forwarded/freed keys array. Mirrors the
        // module-var data-table and string-handle registrations above (this
        // global holds a *raw* I64 pointer, which `mark_global_root_bits` and
        // the evacuation `try_rewrite_value` raw fallback already handle).
        let addr_i64 = blk.ptrtoint(&global_ref, I64);
        blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);

        // #6759 C3 rung 2: mint the canonical ShapeId beside the canonical
        // keys array. Every compiled `new C()` path loads this immutable u32
        // and writes it into the receiver's shape word at birth. The keys
        // global is registered first, so the shape record and every future
        // instance refer to the rooted/rewriteable canonical array.
        // The static birth id names the full (keys, live bound, class/proto,
        // rep) content. The same rep is passed to a lazy mint when no static
        // id is available, so every birth route installs the same facts.
        let shape_id = if requested != "0" {
            // Design step 4: the per-class mint with the driver's static id.
            // Class registration precedes every instance, so this is the first
            // mint of these facts in the agent unless an importing module's
            // own mint of the same content (same id) already ran.
            let live = if birth.wide_live > 0 {
                birth.wide_live
            } else {
                *field_count
            };
            blk.call(
                I32,
                "js_object_shape_id_for_class_keys_static",
                &[
                    (I64, &arr),
                    (I32, &fc_str),
                    (I32, &live.to_string()),
                    (I32, &mint_cid_str),
                    (I32, &requested),
                    (I64, &rep_str),
                ],
            )
        } else {
            // The class id rides along: a birth shape names the prototype
            // its class implies ([[Prototype]] is a shape fact).
            // A class born WIDE (constructor key-add slack) gets a birth
            // shape whose live bound is the widened slot count its header
            // image allocates (`codegen/mod.rs`, `birth_live`).
            match birth.wide_live {
                birth_live if birth_live > 0 => blk.call(
                    I32,
                    "js_object_shape_id_for_class_keys_live",
                    &[
                        (I64, &arr),
                        (I32, &fc_str),
                        (I32, &birth_live.to_string()),
                        (I32, &mint_cid_str),
                        (I64, &rep_str),
                    ],
                ),
                _ => blk.call(
                    I32,
                    "js_object_shape_id_for_class_keys",
                    &[
                        (I64, &arr),
                        (I32, &fc_str),
                        (I32, &mint_cid_str),
                        (I64, &rep_str),
                    ],
                ),
            }
        };
        let shape_global = format!(
            "@{}",
            crate::typed_shape::shape_id_global_name_from_keys_global(global_name)
        );
        blk.store(I32, &shape_id, &shape_global);

        // #8122: compose the class's inline-`new` header image —
        // `[packed GcHeader word | class_id | ShapeId << 32]` — beside the
        // ShapeId it consumes, ONCE. Every inline allocation of this class
        // then stores its 16-byte header prefix with one `<2 x i64>` store
        // instead of composing the pair per site (or per call in a recursive
        // allocator). The packed word came from
        // `target_layout::inline_alloc_gc_packed`, the same derivation the
        // site performs and cross-checks before it trusts this global.
        if let Some(&(image_class_id, gc_packed, _)) = class_header_image_inits.get(global_name) {
            let image_global = format!(
                "@{}",
                crate::typed_shape::header_image_global_name_from_keys_global(global_name)
            );
            let shape_i64 = blk.zext(I32, &shape_id, I64);
            let shape_shifted = blk.shl(I64, &shape_i64, "32");
            let header_word = blk.or(I64, &shape_shifted, &image_class_id.to_string());
            let image = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = insertelement <2 x i64> <i64 {}, i64 0>, i64 {}, i32 1",
                image, gc_packed, header_word
            ));
            blk.emit_raw(format!(
                "store <2 x i64> {}, ptr {}, align 8",
                image, image_global
            ));
        }
    }

    // Register the parent-class chain for every class with a parent.
    // The runtime allocators do this on every alloc; the inline
    // bump allocator skips it. Without this one-time call, the
    // CLASS_REGISTRY misses the `child → parent` edge and walks of
    // the inheritance chain (e.g. `instanceof Shape` on a `Square`
    // where `Square extends Rectangle extends Shape`) terminate
    // prematurely. We emit one call per inheriting class, sorted by
    // class id for deterministic ordering.
    chunker.literals = false;
    let mut parent_pairs: Vec<(u32, u32)> = Vec::new();
    for (name, &cid) in class_ids.iter() {
        if let Some(class) = classes.get(name) {
            if let Some(parent_name) = &class.extends_name {
                if let Some(&parent_cid) = class_ids.get(parent_name) {
                    if parent_cid != 0 {
                        parent_pairs.push((cid, parent_cid));
                    }
                } else if let Some(reserved) =
                    crate::expr::builtin_parent_reserved_class_id(parent_name)
                {
                    // `class S extends Array {}` — the parent is a built-in
                    // with a reserved runtime class id, not a user class. Wire
                    // the edge so `new S() instanceof Array` walks the chain to
                    // the reserved id and matches. Refs class/subclass-builtins.
                    parent_pairs.push((cid, reserved));
                }
            }
        }
    }
    parent_pairs.sort_unstable();
    for (cid, parent_cid) in parent_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        blk.call_void(
            "js_register_class_parent",
            &[(I32, &cid.to_string()), (I32, &parent_cid.to_string())],
        );
    }

    // Issue #392: register every user class method in the runtime
    // VTABLE_REGISTRY so cross-module callers can dispatch via
    // `js_native_call_method` even when the codegen of the calling
    // module can't see the class definition. Same-module calls
    // already resolve through the static idispatch tower in
    // `lower_call.rs` (which iterates `ctx.classes` to find
    // implementors); cross-module calls fall through to
    // `js_native_call_method`, which reads the receiver's class_id
    // and looks up the vtable.
    //
    // Only register classes DEFINED in this module — `class_ids` may
    // include imported classes (Changeset imported from `shared.ts`
    // into `main.ts` for `new Changeset()`), but the `perry_method_*`
    // symbols for those live in the defining module's object file.
    // Each module's init registers its own classes; the linker
    // ensures all init functions run before main.
    // (class_id, name, llvm_symbol, total_param_count, has_synth_args,
    // has_rest, spec_length, definition_order)
    let mut method_triples: Vec<(u32, String, String, u32, bool, bool, u32, u32)> = Vec::new();
    // #1788: (cid, static-method name, perry_static_* symbol, param_count,
    // has_rest). Registered into the runtime CLASS_STATIC_METHODS table so a
    // subclass whose parent is a class-expression value inherits the parent's
    // static methods (`class Sub extends make(...) {}; Sub.greet()`); has_rest
    // tells the dispatcher to bundle trailing args for a `...rest` param.
    #[allow(clippy::type_complexity)]
    let mut static_method_triples: Vec<(u32, String, String, u32, bool, u32, u32, bool, bool)> =
        Vec::new();
    let mut computed_static_entries: Vec<StaticMethodEntry> = Vec::new();
    // #1787: (cid, standalone-constructor symbol, total_param_count).
    // Registered into CLASS_CONSTRUCTORS so `new <classObjectValue>()` (a
    // class-expression value constructed dynamically) can replay the class's
    // constructor + field initializers on the new instance. Only consulted by
    // the heap-class-object arm of `js_new_function_construct`, so it's
    // behavior-neutral for top-level class declarations (INT32 ref `new`).
    // (cid, symbol, total_param_count, sig_cap_count). #5957: `sig_cap_count`
    // is how many TRAILING params are synthesized `__perry_cap_*` capture
    // params IN THE SIGNATURE — the runtime construct dispatchers split the
    // user/cap boundary from this (signature truth), not the decl-site snapshot
    // length, which mis-split dynamic-parent (capless-sig-with-snapshot) ctors.
    let mut ctor_triples: Vec<(u32, String, u32, u32)> = Vec::new();
    // Per-class-id ctor synth/rest flags (has_synthetic_arguments, has_rest)
    // so the `super(...spread)` runtime apply path packs a pass-through parent
    // ctor's `arguments` / rest slot correctly (a zero-declared-param parent
    // that reads `arguments`, e.g. tsc's emitted pass-through ctor), plus the
    // rest param's position (in USER params, -1 when none) so a member-new
    // (`new ns.Sub(opts)` → js_new_function_construct → js_native_call_value)
    // BUNDLES trailing args into the rest array (#wall3). Without that the
    // rest param binds to the first arg as a scalar (a=opts, not [opts]) and
    // `super(...args)` spreads a bare object → 0x400000000 mis-box → crash
    // (Next.js `new c.AppPageRouteModule({...})`). A fact of the class's
    // constructor table, keyed by class id like every other one.
    let mut ctor_flag_regs: Vec<(u32, bool, bool, i64)> = Vec::new();
    for &(cid, class) in &local_classes {
        let class_name = &class.name;
        for method in &class.methods {
            let llvm_name = format!(
                "perry_method_{}__{}__{}",
                module_prefix,
                sanitize_member(class_name),
                sanitize_member(&method.name),
            );
            let has_synth_args = method
                .params
                .last()
                .map(|p| p.arguments_object.is_some())
                .unwrap_or(false);
            // A trailing user rest param (`method(a, ...rest)`) — distinct from
            // the synthesized-`arguments` param above. The runtime needs this
            // so an apply/dynamic dispatch (`recv.method(...spread)`) bundles
            // the call args into the rest array instead of passing `rest =
            // args[0]` as a scalar (marked's `this.use(...e)` blocker).
            // A method that reads `arguments` after declaring `...rest` has
            // two trailing array parameters in HIR: `[...rest, arguments]`.
            // Looking only at the final (synthetic) slot loses the user-rest
            // bit, so bound/runtime vtable dispatch packs a single array and
            // binds the first scalar argument directly to `rest`.
            let has_rest = method
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            // Spec `.length`: count leading formal params before the first one
            // with a default or rest (and excluding the synthesized `arguments`
            // slot). Distinct from the total param_count used for call dispatch.
            let mut spec_length = 0u32;
            for p in &method.params {
                if p.arguments_object.is_some() || p.is_rest || p.default.is_some() {
                    break;
                }
                spec_length += 1;
            }
            method_triples.push((
                cid,
                method.name.clone(),
                llvm_name,
                method.params.len() as u32,
                has_synth_args,
                has_rest,
                spec_length,
                method.id,
            ));
        }
        // #1788: static methods are emitted as `perry_static_*` (no `this`
        // param). Collect them for the runtime CLASS_STATIC_METHODS table.
        for sm in &class.static_methods {
            // A `static { }` block is lowered to a synthetic static method the
            // class's initializer calls directly. It is not a member: never
            // registered, so no reflection (`Reflect.ownKeys(C)`) can see it.
            if sm.name.starts_with("__perry_static_init_") {
                continue;
            }
            let llvm_name = scoped_static_method_name(module_prefix, cid, class_name, &sm.name);
            let has_rest = sm.params.last().map(|p| p.is_rest).unwrap_or(false);
            // Spec `.length`: leading formal params before the first default/rest
            // (and excluding the synthesized `arguments` slot). For static
            // generator/async methods the raw param_count over-counts (`static
            // *gen(a, b = 1,).length === 1`, not 2). (Test262 *-method-static
            // dflt-params-trailing-comma / -length.)
            let mut spec_length = 0u32;
            for p in &sm.params {
                if p.arguments_object.is_some() || p.is_rest || p.default.is_some() {
                    break;
                }
                spec_length += 1;
            }
            static_method_triples.push((
                cid,
                sm.name.clone(),
                llvm_name,
                sm.params.len() as u32,
                has_rest,
                spec_length,
                sm.id,
                sm.params
                    .iter()
                    .any(|p| p.is_rest && p.arguments_object.is_none()),
                sm.params.iter().any(|p| p.arguments_object.is_some()),
            ));
        }
        // A computed-name static method is a ClassBody static method too: its
        // own function object runs the same closure-convention entry. Its
        // key (and so its `name`) exists only when the class definition
        // evaluates, which registers the entry with it
        // (`js_register_class_computed_method`).
        for member in class
            .computed_members
            .iter()
            .filter(|m| m.is_static && matches!(m.kind, perry_hir::ClassComputedMemberKind::Method))
        {
            let f = &member.function;
            let mut spec_length = 0u32;
            for p in &f.params {
                if p.arguments_object.is_some() || p.is_rest || p.default.is_some() {
                    break;
                }
                spec_length += 1;
            }
            computed_static_entries.push(StaticMethodEntry {
                cid,
                llvm_name: scoped_static_method_name(module_prefix, cid, class_name, &f.name),
                param_count: f.params.len() as u32,
                spec_length,
                has_user_rest: f
                    .params
                    .iter()
                    .any(|p| p.is_rest && p.arguments_object.is_none()),
                has_synth_args: f.params.iter().any(|p| p.arguments_object.is_some()),
                home: fresh_class_templates.contains(class_name),
            });
        }
        // #1787: the standalone constructor `<prefix>__<class>_constructor`
        // (emitted unconditionally in `artifacts.rs`). Its arity is the
        // constructor's full param list — user params plus the synthesized
        // `__perry_cap_<id>` capture params (`synthesize_class_captures`).
        // Class-expression templates with no own/synthesized constructor (no
        // captures) have arity 0 — the standalone ctor then just runs the
        // literal field initializers.
        // Wall 51: prefer the synthesized-ctor arity override (which walks the
        // ancestor chain for a no-own-ctor class with heritage, incl.
        // cross-module parents) so the registered `total_params` matches the
        // standalone ctor function actually emitted in `artifacts.rs`. Falls
        // back to the own-ctor param count for classes not in the map.
        let ctor_params = ctor_arity_overrides
            .get(class_name)
            .copied()
            .unwrap_or_else(|| {
                class
                    .constructor
                    .as_ref()
                    .map(|c| c.params.len() as u32)
                    .unwrap_or(0)
            });
        let ctor_symbol = format!("{}__{}_constructor", module_prefix, class_name);
        // The trailing-array layout of the emitted standalone ctor. A class with
        // no own ctor emits the `super(...args)` forwarder, which adopts the
        // nearest local ancestor ctor's params positionally and hands every slot
        // to that ctor unchanged, so it takes the same layout (#10484: a dynamic
        // `new Sub(x)` must put all args in the ancestor's `arguments` slot).
        let shape_params = constructor_layout_params(class, classes, ctor_params);
        // #wall3: the rest-param position (in USER params), registered with
        // the flags below.
        let ctor_rest_fixed = shape_params
            .and_then(|params| params.iter().position(|p| p.is_rest))
            .map_or(-1, |rest_idx| rest_idx as i64);
        // Record the ctor's trailing-param shape so the `super(...spread)`
        // apply path forwards the flat spread args and packs the trailing slot:
        // a synthesized `arguments` slot receives ALL args (from index 0), a
        // user rest param only the args from the rest position onward.
        {
            // `any`, not `last`: a class declared inside a function carries
            // synthesized `__perry_cap_*` params AFTER the `arguments` slot, so
            // reading only the final param missed every capturing class —
            // including every class in a compiled CommonJS module, whose
            // wrapper function is what they capture from (#10484).
            let ctor_has_synth = shape_params
                .map(|params| params.iter().any(|p| p.arguments_object.is_some()))
                .unwrap_or(false);
            let ctor_has_rest = shape_params
                .map(|params| {
                    params
                        .iter()
                        .any(|p| p.is_rest && p.arguments_object.is_none())
                })
                .unwrap_or(false);
            if ctor_has_synth || ctor_has_rest || ctor_rest_fixed >= 0 {
                ctor_flag_regs.push((cid, ctor_has_synth, ctor_has_rest, ctor_rest_fixed));
            }
        }
        // #5957: count the ctor's trailing `__perry_cap_*` signature params.
        // An arity-override (no own ctor) class is capture-free by the
        // synthesized-ctor invariant → 0; a function-nested class that captures
        // has its cap params appended by `synthesize_class_captures` and they
        // are reflected in `class.constructor` at codegen time.
        let ctor_sig_caps = class
            .constructor
            .as_ref()
            .map(|c| {
                c.params
                    .iter()
                    .filter(|p| p.name.starts_with("__perry_cap_"))
                    .count() as u32
            })
            .unwrap_or(0);
        ctor_triples.push((cid, ctor_symbol, ctor_params, ctor_sig_caps));
    }
    let fresh_cids: std::collections::HashSet<u32> = fresh_class_templates
        .iter()
        .filter_map(|name| class_ids.get(name).copied())
        .collect();
    // Each per-evaluation template's own record, its template cell
    // (`fresh_class_templates::template_cell_global`). Every class object of
    // the template names it (its first own key), so the runtime paths that
    // start from one find it there.
    let mut fresh_cells: Vec<(u32, usize)> = fresh_class_templates
        .iter()
        .filter_map(|name| {
            let cid = *class_ids.get(name)?;
            let words =
                super::fresh_class_templates::template_cell_words(classes.get(name).copied());
            Some((cid, words))
        })
        .collect();
    fresh_cells.sort_unstable();
    fresh_cells.dedup_by_key(|cell| cell.0);
    for (cid, words) in fresh_cells {
        let global = super::fresh_class_templates::template_cell_global(cid);
        chunker.module().add_raw_global(format!(
            "@{global} = internal global [{words} x i64] [i64 {words}{}]",
            ", i64 0".repeat(words - 1)
        ));
    }
    method_triples.sort_unstable();
    let mut method_entries: Vec<StaticMethodEntry> = Vec::new();
    for (
        cid,
        method_name,
        llvm_name,
        param_count,
        has_synth_args,
        has_rest,
        spec_length,
        definition_order,
    ) in method_triples
    {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        // The pre-intern pass before `emit_string_pool` ensured every
        // method name has a string pool entry; look it up here without
        // mutating the pool.
        let entry = match strings.lookup(&method_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        // Every class prototype holds a function object of each method's own
        // body, running this closure-convention entry: its JsFunctionInfo is the
        // ConstFn fact the prototype's shape records for the method's slot. A
        // per-evaluation template's evaluations each hold their own object, at
        // home in that evaluation (`class_method_entry_enter_home`); a declared
        // class's one object has no home and runs in its receiver's evaluation,
        // exactly as a vtable call does.
        // The entries are defined after every init chunk (below), so the
        // init code module init runs stays contiguous: an entry runs only
        // when its method value is called.
        let home = fresh_cids.contains(&cid);
        let entry_name = format!("{}__eclo", llvm_name);
        let entry_ref = format!("@{}", entry_name);
        method_entries.push(StaticMethodEntry {
            cid,
            llvm_name: llvm_name.clone(),
            param_count,
            spec_length,
            has_user_rest: has_rest,
            has_synth_args,
            home,
        });
        let entry_info_ref = blk.fn_info_ref(&entry_name);
        // Cast the method function pointer to i64 via ptrtoint so the
        // runtime can store it as a `usize` in the VTABLE_REGISTRY
        // entry. The `inttoptr` round-trip in `call_vtable_method`
        // restores it for the indirect call.
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        let entry_i64 = blk.ptrtoint(&entry_info_ref, I64);
        let has_synth_args_str = if has_synth_args { "1" } else { "0" };
        let has_rest_str = if has_rest { "1" } else { "0" };
        // One registration per method carries its entry. A declared class's
        // method object names its entry's code when it is first built, so the
        // init pays nothing more per method; each evaluation of a template
        // builds its own objects from the template, so the name is
        // registered here.
        blk.call_void(
            "js_register_class_method_with_entry",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
                (I64, &param_count.to_string()),
                (I64, has_synth_args_str),
                (I64, has_rest_str),
                (I64, &entry_i64),
            ],
        );
        if home {
            blk.call_void(
                register_name_fn,
                &[(PTR, &entry_ref), (PTR, &bytes_global), (I32, &len_str)],
            );
        }
        blk.call_void(
            "js_register_class_string_member_order",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, "0"),
                (I64, &definition_order.to_string()),
            ],
        );
        // Record the default-aware spec `.length` so `C.prototype.m.length`
        // reflects params-before-first-default, not the raw param count.
        blk.call_void(
            "js_register_class_method_bind_length",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &spec_length.to_string()),
            ],
        );
    }
    // #1788: register static methods into CLASS_STATIC_METHODS so inherited
    // static methods (subclass extends a class-expression value) resolve at
    // runtime via the class_id parent-chain walk.
    static_method_triples.sort_unstable();
    for (
        cid,
        method_name,
        llvm_name,
        param_count,
        has_rest,
        spec_length,
        definition_order,
        has_user_rest,
        has_synth_args,
    ) in static_method_triples
    {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.lookup(&method_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        let has_rest_str = if has_rest { "1" } else { "0" };
        blk.call_void(
            "js_register_class_static_method",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
                (I64, &param_count.to_string()),
                (I64, has_rest_str),
            ],
        );
        blk.call_void(
            "js_register_class_string_member_order",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, "1"),
                (I64, &definition_order.to_string()),
            ],
        );
        // Record the default-aware spec `.length` for the static method so
        // `C.staticGen.length` reflects params-before-first-default rather than
        // the raw param count (which over-counts generator/async methods).
        blk.call_void(
            "js_register_class_static_method_bind_length",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &spec_length.to_string()),
            ],
        );
        let (entry_ref, entry_info_ref) = emit_static_method_entry(
            &mut chunker,
            &StaticMethodEntry {
                cid,
                llvm_name: llvm_name.clone(),
                param_count,
                spec_length,
                has_user_rest,
                has_synth_args,
                home: fresh_cids.contains(&cid),
            },
        );
        let blk = chunker.current_block();
        blk.call_void(
            register_name_fn,
            &[(PTR, &entry_ref), (PTR, &bytes_global), (I32, &len_str)],
        );
        let entry_i64 = blk.ptrtoint(&entry_info_ref, I64);
        blk.call_void(
            "js_register_class_static_method_entry",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &entry_i64),
            ],
        );
    }
    computed_static_entries.sort_unstable_by(|a, b| a.llvm_name.cmp(&b.llvm_name));
    for e in &computed_static_entries {
        chunker.roll_if_full();
        let _ = emit_static_method_entry(&mut chunker, e);
    }
    // #1787: register each class's standalone constructor into
    // CLASS_CONSTRUCTORS. ptrtoint @symbol both stores the function pointer
    // and keeps the constructor alive past dead-code elimination.
    ctor_triples.sort_unstable();
    for (cid, ctor_symbol, ctor_params, ctor_sig_caps) in ctor_triples {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", ctor_symbol);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        blk.call_void(
            "js_register_class_constructor",
            &[
                (I64, &cid.to_string()),
                (I64, &func_i64),
                (I64, &ctor_params.to_string()),
                (I64, &ctor_sig_caps.to_string()),
            ],
        );
    }
    // Register ctor synth/rest flags so `super(...spread)` packs the parent
    // ctor's trailing `arguments` / rest slot correctly.
    ctor_flag_regs.sort_unstable();
    for (cid, has_synth, has_rest, rest_fixed) in ctor_flag_regs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        blk.call_void(
            "js_register_class_constructor_flags",
            &[
                (I64, &cid.to_string()),
                (I64, if has_synth { "1" } else { "0" }),
                (I64, if has_rest { "1" } else { "0" }),
                (I64, &rest_fixed.to_string()),
            ],
        );
    }

    // Refs #618 / #420: register every class id with the runtime so
    // `js_value_typeof` can distinguish a class ref (NaN-boxed INT32 with
    // class_id payload) from a real int32 numeric value. Without this,
    // `typeof <class>` returns "number" for classes that don't define any
    // methods (the existing `js_register_class_method` loop only fires
    // for classes with at least one method body). drizzle's `class
    // FakePrimitiveParam { static [entityKind] = "FakePrimitiveParam" }`
    // and similar method-less marker classes hit this.
    {
        let mut all_class_ids: Vec<u32> = Vec::new();
        // Also collect `(cid, name)` pairs so we can mirror Perry's
        // user-visible class name into the runtime — V8 reads it back as
        // `metatype.name` (#1021 NestJS module token factory).
        let mut named_classes: Vec<(u32, String)> = Vec::new();
        for (class_name, class) in classes.iter() {
            if *class_name != class.name {
                continue;
            }
            let cid = match class_ids.get(class_name).copied() {
                Some(c) if c != 0 => c,
                _ => continue,
            };
            all_class_ids.push(cid);
            // date-fns / drizzle / lodash plain-object duck-checks need
            // `({ x: 1 }).constructor === Object` to hold. The HIR
            // synthesizes an `__AnonShape_<hash>` class per literal
            // shape; mark each such class id so the runtime
            // `js_object_get_field_by_name` resolves `.constructor` to
            // the global `Object` constructor instead of the synthetic
            // class ref.
            // Anon-shape ids are registered BEFORE the class ShapeIds are
            // minted (#11420, above the keys loop), not here.
            if !class_name.starts_with("__AnonShape_") {
                named_classes.push((cid, class_name.clone()));
            }
        }
        all_class_ids.sort_unstable();
        all_class_ids.dedup();
        for cid in all_class_ids {
            chunker.roll_if_full();
            let blk = chunker.current_block();
            blk.call_void(
                "js_register_class_id",
                &[(crate::types::I32, &cid.to_string())],
            );
        }
        // (Class-name registration uses pre-allocated string constants — see
        // `named_class_name_constants` below.) Drop the named_classes
        // collection here since the pre-computed list is what we'll emit.
        let _ = named_classes;
    }
    // Mirror class names into the runtime so the V8 bridge can surface them
    // as `metatype.name`. Strings were pre-allocated above before `init_fn`
    // borrowed `llmod`. (#1021 NestJS.)
    for (cid, const_name, byte_len) in &named_class_name_constants {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let const_ref = format!("@{}", const_name);
        blk.call_void(
            "js_register_class_name",
            &[
                (crate::types::I32, &cid.to_string()),
                (crate::types::PTR, &const_ref),
                (crate::types::I32, &byte_len.to_string()),
            ],
        );
    }
    // #9413: mirror each class's retained source text into the runtime so
    // `Function.prototype.toString` on a class REF (an INT32 immediate, not a
    // ClosureHeader) answers with the class source. Same shape, and the same
    // output-kind spelling choice, as the function-source loop above (#11501:
    // an executable lends its rodata instead of copying every class body).
    for (cid, source) in &class_source_constants {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let const_ref = source.pointer(blk);
        blk.call_void(
            register_class_source_fn,
            &[
                (crate::types::I32, &cid.to_string()),
                (crate::types::PTR, &const_ref),
                (crate::types::I32, &source.byte_len.to_string()),
            ],
        );
    }
    // Class refs are immediate values, not heap Function objects. Register
    // each constructor's visible arity so the field-get path can reify the
    // Function-compatible own `length` property.
    let mut class_lengths: Vec<(u32, u32)> = classes
        .iter()
        .filter(|(class_name, class)| *class_name == &class.name && class.id != 0)
        .filter_map(|(class_name, class)| {
            let cid = class_ids.get(class_name).copied()?;
            let length = class
                .constructor
                .as_ref()
                .map(|ctor| {
                    ctor.params
                        .iter()
                        .take_while(|p| {
                            !p.is_rest && p.default.is_none() && !p.name.starts_with("__perry_cap_")
                        })
                        .count() as u32
                })
                .unwrap_or(0);
            Some((cid, length))
        })
        .collect();
    class_lengths.sort_unstable_by_key(|(cid, _)| *cid);
    class_lengths.dedup_by_key(|(cid, _)| *cid);
    for (cid, length) in class_lengths {
        chunker.roll_if_full();
        chunker.current_block().call_void(
            "js_register_class_length",
            &[
                (crate::types::I32, &cid.to_string()),
                (crate::types::I32, &length.to_string()),
            ],
        );
    }

    // Refs #486 (hono logger middleware): also register every class
    // getter in the runtime VTABLE_REGISTRY. Without this, cross-module
    // `obj.prop` reads (where `obj` is statically typed `any` so the
    // codegen has no static dispatch info) fall through `js_get_object_field_by_name`
    // past the `vtable.getters.get(prop)` lookup at value.rs:2268 — the
    // map is always empty — and into the field-by-name dispatcher,
    // which returns `undefined` for properties that exist only as
    // getters. Hono's `Context.get req()` is the canonical breakage:
    // the logger middleware reads `c.req.url` from a JS-bundled hono
    // dist via `compilePackages`, and pre-fix `c.req` always returned
    // `undefined`.
    // (class_id, prop_name, llvm_symbol, is_static) — static accessors register
    // onto the class constructor (CLASS_STATIC_ACCESSORS), not the instance vtable.
    let mut getter_pairs: Vec<(u32, String, String, bool, u32)> = Vec::new();
    for &(cid, class) in &local_classes {
        let class_name = &class.name;
        for (prop, getter_fn) in &class.getters {
            // The local-emit path at codegen.rs:1858 prepends `__get_`
            // to the HIR-assigned getter name (`get_<prop>`), giving
            // the LLVM symbol `perry_method_<modprefix>__<class>__<sanitize(__get_get_<prop>)>`.
            // Use the same mangling here so the registered func_ptr
            // matches the actual emitted body.
            let is_static = class.static_accessor_fn_ids.contains(&getter_fn.id);
            // Static accessors are emitted via compile_static_method (no-this
            // ABI) under a `perry_static_…` symbol keyed on `__get_<prop>`;
            // instance accessors via compile_method under `perry_method_…`.
            let llvm_name = if is_static {
                super::helpers::scoped_static_method_name(
                    module_prefix,
                    cid,
                    class_name,
                    &format!("__get_{}", prop),
                )
            } else {
                let inner = format!("__get_{}", getter_fn.name);
                format!(
                    "perry_method_{}__{}__{}",
                    module_prefix,
                    sanitize_member(class_name),
                    sanitize_member(&inner),
                )
            };
            getter_pairs.push((cid, prop.clone(), llvm_name, is_static, getter_fn.id));
        }
    }
    getter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name, is_static, definition_order) in getter_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.lookup(&prop_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        let register_fn = if is_static {
            "js_register_class_static_getter"
        } else {
            "js_register_class_getter"
        };
        blk.call_void(
            register_fn,
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
            ],
        );
        blk.call_void(
            "js_register_class_string_member_order",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, if is_static { "1" } else { "0" }),
                (I64, &definition_order.to_string()),
            ],
        );
    }

    // Refs #486 (hono): parallel registration for class setters. Without
    // this, `c.res = response` (where `c` is `any`-typed) bypasses hono
    // Context's `set res(_res) { …; this.finalized = true; }` and writes
    // directly to a regular field slot. `this.finalized = true` never
    // executes, hono-base sees `c.finalized = false` and throws "Context
    // is not finalized" on every request through compose. Mirror's the
    // getter-pairs loop above; emission mangling matches the
    // setter-method-emission path at codegen.rs:2041 (renamed.name =
    // "__set_<prop>" → LLVM symbol perry_method_<mp>__<class>____set_<f.name>).
    // (cid, prop, llvm_name, is_static, spec_length). `spec_length` is the
    // ECMAScript-visible `.length` of the setter function value read via
    // `Object.getOwnPropertyDescriptor(proto, prop).set` — leading formal
    // params before the first default/rest. A setter always has one formal
    // param, but `set m(x = 42)` has `.length === 0` (test262
    // class/setter-length-dflt): without a per-func-ptr length registration
    // the runtime fell back to the setter's ABI arity (1), over-counting the
    // defaulted param.
    let mut setter_pairs: Vec<(u32, String, String, bool, u32, u32)> = Vec::new();
    for &(cid, class) in &local_classes {
        let class_name = &class.name;
        for (prop, setter_fn) in &class.setters {
            let is_static = class.static_accessor_fn_ids.contains(&setter_fn.id);
            let llvm_name = if is_static {
                super::helpers::scoped_static_method_name(
                    module_prefix,
                    cid,
                    class_name,
                    &format!("__set_{}", prop),
                )
            } else {
                let inner = format!("__set_{}", setter_fn.name);
                format!(
                    "perry_method_{}__{}__{}",
                    module_prefix,
                    sanitize_member(class_name),
                    sanitize_member(&inner),
                )
            };
            let spec_length = spec_function_length(&setter_fn.params) as u32;
            setter_pairs.push((
                cid,
                prop.clone(),
                llvm_name,
                is_static,
                spec_length,
                setter_fn.id,
            ));
        }
    }
    setter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name, is_static, spec_length, definition_order) in setter_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.lookup(&prop_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        // The setter's spec `.length` rides with its registration, so
        // `Object.getOwnPropertyDescriptor(proto, prop).set.length` reports
        // the default-aware count instead of the raw ABI arity.
        let register_fn = if is_static {
            "js_register_class_static_setter"
        } else {
            "js_register_class_setter"
        };
        blk.call_void(
            register_fn,
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
                (I32, &spec_length.to_string()),
            ],
        );
        blk.call_void(
            "js_register_class_string_member_order",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, if is_static { "1" } else { "0" }),
                (I64, &definition_order.to_string()),
            ],
        );
    }

    // Final class records follow all class/prototype registrations. They
    // coexist with the ordinary allocation ids and never feed header images.
    if super::static_constfn::has_final_shapes() {
        let defined_classes: HashMap<_, _> = module_classes
            .iter()
            .filter_map(|class| class_ids.get(&class.name).map(|cid| (*cid, class)))
            .collect();
        for entry in class_keys_init_data {
            let birth = super::static_shape_ids::class_birth(
                module_prefix,
                entry,
                class_header_image_inits,
                class_birth_reps,
                class_ids,
            );
            let Some(ordinary) = birth.shape else {
                continue;
            };
            let Some(class) = defined_classes.get(&birth.class_id).copied() else {
                continue;
            };
            let Ok(shape) = super::static_constfn_class::class_final(
                module_prefix,
                class,
                classes,
                ordinary.rep,
                birth.class_id,
            ) else {
                continue;
            };
            if shape.keys != ordinary.keys
                || shape.live != ordinary.live
                || shape.proto != ordinary.proto
            {
                continue;
            }
            let Some(id) = super::static_shape_ids::static_final_shape_id(&shape) else {
                continue;
            };
            chunker.roll_if_full();
            let blk = chunker.current_block();
            // Registration calls above can collect; load the canonical keys
            // afresh from their registered root immediately before the mint.
            let keys = blk.load(I64, &format!("@{}", entry.0));
            blk.call(
                I32,
                "js_object_final_shape_id_for_class_keys_static_constfn",
                &[
                    (I64, &keys),
                    (I32, &shape.key_count.to_string()),
                    (I32, &shape.live.to_string()),
                    (I32, &birth.class_id.to_string()),
                    (I32, &id.to_string()),
                    (I64, &shape.rep.to_string()),
                    (
                        PTR,
                        &format!(
                            "@{}",
                            super::static_constfn::entries_symbol(module_prefix, id)
                        ),
                    ),
                    (I32, &shape.constfn.len().to_string()),
                ],
            );
        }
    }

    // Completed private contents (#11791, `static_private_class`): minted by
    // facts under their static ids after every class/prototype registration,
    // before any instance can reach them.
    if super::static_shape_ids::has_static_final_shapes() {
        let defined_classes: HashMap<_, _> = module_classes
            .iter()
            .filter_map(|class| class_ids.get(&class.name).map(|cid| (*cid, class)))
            .collect();
        for entry in class_keys_init_data {
            let birth = super::static_shape_ids::class_birth(
                module_prefix,
                entry,
                class_header_image_inits,
                class_birth_reps,
                class_ids,
            );
            let Some(ordinary) = birth.shape else {
                continue;
            };
            let Some(class) = defined_classes.get(&birth.class_id).copied() else {
                continue;
            };
            let Some(shape) =
                super::static_private_class::private_final(class, classes, &ordinary, &|name| {
                    super::static_private_class::element_class_id_in(classes, class_ids, name)
                })
            else {
                continue;
            };
            let Some(id) = super::static_shape_ids::static_final_shape_id(&shape) else {
                continue;
            };
            super::static_private_class::emit_final_constants(
                chunker.module(),
                module_prefix,
                &shape,
                id,
            );
            let symbol = super::static_private_class::final_symbol(module_prefix, id);
            let private_ptr = if shape.private.is_empty() {
                "null".to_string()
            } else {
                format!("@{symbol}_keys")
            };
            let brands_ptr = if shape.brands.is_empty() {
                "null".to_string()
            } else {
                format!("@{symbol}_brands")
            };
            chunker.roll_if_full();
            let blk = chunker.current_block();
            // Registration calls above can collect; load the canonical keys
            // afresh from their registered root immediately before the mint.
            let keys = blk.load(I64, &format!("@{}", entry.0));
            blk.call(
                I32,
                "js_object_final_shape_id_for_class_keys_static_private",
                &[
                    (I64, &keys),
                    (I32, &shape.key_count.to_string()),
                    (I32, &shape.live.to_string()),
                    (I32, &birth.class_id.to_string()),
                    (I32, &id.to_string()),
                    (I64, &shape.rep.to_string()),
                    (PTR, &private_ptr),
                    (I32, &shape.private.len().to_string()),
                    (PTR, &brands_ptr),
                    (I32, &shape.brands.len().to_string()),
                ],
            );
        }
    }

    for e in &method_entries {
        emit_class_method_entry(&mut chunker, e);
    }
    let [literal_chunks, class_chunks] = chunker.finish();
    record_fn_info_facts(
        llmod,
        module_prefix,
        FnInfoFactSources {
            closure_rest_params,
            closure_arities,
            closure_lengths,
            closure_arrow_functions,
            trusted_box_closures,
            versioned_loop_callbacks,
            user_fn_wrapper_rest,
            closure_synthetic_arguments,
            user_fn_wrapper_synthetic_arguments,
            closure_rest_and_arguments,
            user_fn_wrapper_rest_and_arguments,
            user_fn_wrapper_arity,
            user_fn_wrapper_length,
            user_fn_wrapper_async,
            user_fn_wrapper_generator,
            user_fn_wrapper_async_generator,
            user_fn_wrapper_strict,
        },
    );
    // A cyclic importer can call a hoisted factory before this module body.
    // Prepare literal strings/function info/ordinary-object layouts first,
    // once per arena. Workers can enter __init_body directly, so the body
    // also reaches this guarded preparation without allocating a second pool.
    let prepare_name = format!("__perry_prepare_literals_{}", module_prefix);
    let prepared = format!("__perry_literals_ready_{}", module_prefix);
    if super::program_has_worker() {
        llmod.add_internal_thread_local_global(&prepared, I8, "0");
    } else {
        llmod.add_internal_global(&prepared, I8, "0");
    }
    let prepare_fn = llmod.define_function(&prepare_name, VOID, vec![]);
    prepare_fn.create_block("entry");
    prepare_fn.create_block("prepare");
    prepare_fn.create_block("done");
    let prepare_label = prepare_fn.block_mut(1).unwrap().label.clone();
    let done_label = prepare_fn.block_mut(2).unwrap().label.clone();
    let blk = prepare_fn.block_mut(0).unwrap();
    let ready = blk.load(I8, &format!("@{}", prepared));
    let ready = blk.icmp_ne(I8, &ready, "0");
    blk.cond_br(&ready, &done_label, &prepare_label);
    let blk = prepare_fn.block_mut(1).unwrap();
    blk.call_void(&agent_strings_name, &[]);
    for cname in &literal_chunks {
        blk.call_void(cname, &[]);
    }
    blk.store(I8, "1", &format!("@{}", prepared));
    blk.br(&done_label);
    prepare_fn.block_mut(2).unwrap().ret_void();

    let init_name = format!("__perry_init_strings_{}", module_prefix);
    let init_fn = llmod.define_function(&init_name, VOID, vec![]);
    init_fn.create_block("entry");
    let blk = init_fn.block_mut(0).unwrap();
    blk.call_void(&prepare_name, &[]);
    for cname in &class_chunks {
        blk.call_void(cname, &[]);
    }
    blk.ret_void();
}

/// The codegen metadata a module's `JsFunctionInfo` facts come from
/// (`crate::fn_info`), keyed by closure func-id or wrapper symbol.
struct FnInfoFactSources<'a> {
    closure_rest_params: &'a HashMap<u32, usize>,
    closure_arities: &'a HashMap<u32, u32>,
    closure_lengths: &'a HashMap<u32, u32>,
    closure_arrow_functions: &'a std::collections::HashSet<u32>,
    trusted_box_closures:
        &'a std::collections::HashMap<u32, super::closure_collect::TrustedBoxClosure>,
    versioned_loop_callbacks: &'a std::collections::HashSet<u32>,
    user_fn_wrapper_rest: &'a [(String, usize)],
    closure_synthetic_arguments: &'a std::collections::HashSet<u32>,
    user_fn_wrapper_synthetic_arguments: &'a std::collections::HashSet<String>,
    closure_rest_and_arguments: &'a std::collections::HashSet<u32>,
    user_fn_wrapper_rest_and_arguments: &'a std::collections::HashSet<String>,
    user_fn_wrapper_arity: &'a [(String, u32)],
    user_fn_wrapper_length: &'a [(String, u32)],
    user_fn_wrapper_async: &'a std::collections::HashSet<String>,
    user_fn_wrapper_generator: &'a std::collections::HashSet<String>,
    user_fn_wrapper_async_generator: &'a std::collections::HashSet<String>,
    user_fn_wrapper_strict: &'a std::collections::HashSet<String>,
}

/// Record every fact the module knows about its own closure bodies and
/// value wrappers in their `JsFunctionInfo`s (`crate::fn_info`):
///
/// * rest kind + fixed arity (#493, #653; #915's synthesized `arguments`
///   bundles ALL args; rest-and-arguments passes both arrays) — dynamic
///   dispatch bundles trailing args for call sites that cannot see the
///   body's signature (`obj.cb(a, b, c)` with `cb` a `(...args) => …` field);
/// * declared arity of non-rest bodies (#421) — `fn.length`'s fallback;
/// * ECMAScript `.length` (default-aware; Ramda's `converge` / `juxt` read
///   it from function values);
/// * arrow, strict (#4850 OrdinaryCallBindThis), async, generator and
///   async-generator (#3664) bits;
/// * the trusted direct-call and versioned-loop clones of eligible arrows.
fn record_fn_info_facts(llmod: &LlModule, module_prefix: &str, src: FnInfoFactSources<'_>) {
    use crate::fn_info::RestKind;
    let closure = |fid: u32| format!("perry_closure_{}__{}", module_prefix, fid);
    for (&fid, &fixed) in src.closure_rest_params {
        let kind = if src.closure_rest_and_arguments.contains(&fid) {
            RestKind::UserAndArguments
        } else if src.closure_synthetic_arguments.contains(&fid) {
            RestKind::SyntheticArguments
        } else {
            RestKind::User
        };
        llmod.note_fn_info(&closure(fid), |f| f.set_rest(fixed, kind));
    }
    for (&fid, &arity) in src.closure_arities {
        llmod.note_fn_info(&closure(fid), |f| f.set_declared(arity));
    }
    for (&fid, &length) in src.closure_lengths {
        llmod.note_fn_info(&closure(fid), |f| f.set_length(length));
    }
    for &fid in src.closure_arrow_functions {
        llmod.note_fn_info(&closure(fid), |f| f.set_arrow());
    }
    // The clones hang off arrow bodies only (the runtime's resolvers take
    // them from an arrow's info).
    for (&fid, plan) in src.trusted_box_closures {
        if !src.closure_arrow_functions.contains(&fid) {
            continue;
        }
        let public = closure(fid);
        let trusted = format!("{public}$trusted_boxes");
        let versioned = src
            .versioned_loop_callbacks
            .contains(&fid)
            .then(|| format!("{trusted}$versioned_loop"));
        llmod.note_fn_info(&public, |f| {
            f.trusted = Some(crate::fn_info::CloneTarget {
                symbol: trusted,
                captures: plan.capture_count,
                boxed_mask: plan.boxed_capture_mask,
            });
            f.versioned = versioned.map(|symbol| crate::fn_info::CloneTarget {
                symbol,
                captures: plan.capture_count,
                boxed_mask: plan.boxed_capture_mask,
            });
        });
    }
    let rest_wrappers: std::collections::HashSet<&str> = src
        .user_fn_wrapper_rest
        .iter()
        .map(|(s, _)| s.as_str())
        .collect();
    for (wrap_sym, fixed) in src.user_fn_wrapper_rest {
        let kind = if src.user_fn_wrapper_rest_and_arguments.contains(wrap_sym) {
            RestKind::UserAndArguments
        } else if src.user_fn_wrapper_synthetic_arguments.contains(wrap_sym) {
            RestKind::SyntheticArguments
        } else {
            RestKind::User
        };
        llmod.note_fn_info(wrap_sym, |f| f.set_rest(*fixed, kind));
    }
    for (wrap_sym, arity) in src.user_fn_wrapper_arity {
        if rest_wrappers.contains(wrap_sym.as_str()) {
            continue;
        }
        llmod.note_fn_info(wrap_sym, |f| f.set_declared(*arity));
    }
    for (wrap_sym, length) in src.user_fn_wrapper_length {
        llmod.note_fn_info(wrap_sym, |f| f.set_length(*length));
    }
    for wrap_sym in src.user_fn_wrapper_async {
        llmod.note_fn_info(wrap_sym, |f| f.set_async());
    }
    for wrap_sym in src.user_fn_wrapper_generator {
        llmod.note_fn_info(wrap_sym, |f| f.set_generator());
    }
    for wrap_sym in src.user_fn_wrapper_async_generator {
        llmod.note_fn_info(wrap_sym, |f| f.set_async_generator());
    }
    for wrap_sym in src.user_fn_wrapper_strict {
        llmod.note_fn_info(wrap_sym, |f| f.set_strict());
    }
}

#[cfg(test)]
#[path = "class_name_registration_tests.rs"]
mod class_name_registration_tests;

#[path = "method_entries.rs"]
mod method_entries;
use method_entries::{emit_class_method_entry, emit_static_method_entry, StaticMethodEntry};
