//! String pool emission. Split out of `codegen.rs` (now `codegen/mod.rs`).

use std::collections::HashMap;

use crate::block::LlBlock;
use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

use super::helpers::{sanitize, sanitize_member, scoped_static_method_name};
use super::spec_function_length;

/// Emits a long sequence of INDEPENDENT init operations (string allocation,
/// closure/class/function registration) into a series of small chunk functions
/// instead of one giant function, then exposes the chunk names so the entry
/// `__perry_init_strings_*` can call them in order (#5391 function splitting).
///
/// A large bundle interns ~190K strings and registers tens of thousands of
/// closures/classes; emitting all of it into ONE function produced a single
/// ~32MB / ~400K-instruction basic block, and `clang -O0` (forced for oversized
/// modules, #4880) is catastrophically superlinear on a single huge block
/// (~36 min). Every op here is independent — each writes to its own global or a
/// runtime registry; no SSA value flows between ops — so splitting at op
/// boundaries is safe and order-preserving (chunks run in sequence, ops in order
/// within a chunk).
struct InitChunker<'a> {
    llmod: &'a mut LlModule,
    base_name: String,
    ops_per_chunk: usize,
    ops_in_current: usize,
    cur_idx: usize,
    chunk_names: Vec<String>,
}

impl<'a> InitChunker<'a> {
    fn new(llmod: &'a mut LlModule, base_name: String, ops_per_chunk: usize) -> Self {
        Self {
            llmod,
            base_name,
            ops_per_chunk: ops_per_chunk.max(1),
            // Force a fresh chunk on the first op.
            ops_in_current: usize::MAX,
            cur_idx: 0,
            chunk_names: Vec::new(),
        }
    }

    /// Start a fresh chunk function if the current one is full. Call ONCE at the
    /// top of each loop iteration (one independent init op), before
    /// [`current_block`]. Closes the previous chunk with `ret void`.
    fn roll_if_full(&mut self) {
        if self.ops_in_current >= self.ops_per_chunk {
            if !self.chunk_names.is_empty() {
                self.llmod
                    .function_mut(self.cur_idx)
                    .unwrap()
                    .block_mut(0)
                    .unwrap()
                    .ret_void();
            }
            let name = format!("{}_chunk{}", self.base_name, self.chunk_names.len());
            self.llmod
                .define_function(&name, VOID, vec![])
                .create_block("entry");
            self.cur_idx = self.llmod.function_count() - 1;
            self.chunk_names.push(name);
            self.ops_in_current = 0;
        }
    }

    /// The current chunk's entry block, for emitting one op's instructions.
    /// Counts as one op (a logical init step may emit several instructions onto
    /// it). Always preceded by [`roll_if_full`].
    fn current_block(&mut self) -> &mut LlBlock {
        self.ops_in_current += 1;
        self.llmod
            .function_mut(self.cur_idx)
            .unwrap()
            .block_mut(0)
            .unwrap()
    }

    /// Close the final chunk and return all chunk function names, in order.
    fn finish(self) -> Vec<String> {
        if !self.chunk_names.is_empty() {
            self.llmod
                .function_mut(self.cur_idx)
                .unwrap()
                .block_mut(0)
                .unwrap()
                .ret_void();
        }
        self.chunk_names
    }
}

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
    class_keys_init_data: &[(String, String, u32, Vec<u64>, Vec<u64>)],
    class_ids: &HashMap<String, u32>,
    classes: &HashMap<String, &perry_hir::Class>,
    // #5592: user-visible `.name` overrides keyed by ClassId, for classes
    // whose HIR registration key was uniquified away from their JS name.
    class_display_names: &HashMap<u32, String>,
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
    // Issue #653: wrappers (`__perry_wrap_<name>`) for top-level user functions
    // that declare a rest param. Each entry is `(wrapper_symbol, fixed_arity)`
    // — the runtime side-table is keyed on the wrapper's func_ptr, NOT the
    // underlying user function, because that's what `js_closure_alloc_singleton`
    // stores in the ClosureHeader. Without this registration, calling a user
    // function as a value through `js_closure_call_apply_with_spread` fed the
    // raw spread elements into the wrapper's flat `(this, a0, a1)` signature
    // instead of bundling args[fixed_arity..] into a real array — the rest
    // param then read a single element's bits as if it were the rest array.
    user_fn_wrapper_rest: &[(String, usize)],
    // Refs #915 (gap 1 from #899): subset of `closure_rest_params` whose
    // rest param is the HIR-synthesized `arguments` array. These need
    // `js_register_closure_synthetic_arguments` so the runtime bundles
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
    // (`__perry_wrap_<original_name>`) — used to register the wrapper's
    // declared arity in the runtime's `CLOSURE_ARITY_REGISTRY` so dynamic
    // dispatch can pad missing trailing args before invoking the wrapper.
    // Entries for wrappers also present in `user_fn_wrapper_rest` are skipped
    // (those go through the rest registry which already controls dispatch).
    user_fn_wrapper_arity: &[(String, u32)],
    // ECMAScript-visible `.length` for every top-level user-function wrapper.
    user_fn_wrapper_length: &[(String, u32)],
    // Wrapper symbols for top-level async functions. Registered by function
    // pointer so `util.types.isAsyncFunction` keeps working when the value is
    // observed through a runtime alias instead of direct HIR.
    user_fn_wrapper_async: &std::collections::HashSet<String>,
    // Wrapper/closure symbols whose original source form was a generator
    // function. Registered so util.types.isGeneratorFunction can distinguish
    // lowered generator state-machine closures from ordinary functions.
    user_fn_wrapper_generator: &std::collections::HashSet<String>,
    // #3664: wrapper/closure symbols whose source form was `async function*`.
    // Registered in the runtime's async-generator registry so the
    // `%AsyncGeneratorFunction%`/`%AsyncGenerator%` intrinsic chain (and
    // `util.types.isAsyncFunction`) resolve correctly for them.
    user_fn_wrapper_async_generator: &std::collections::HashSet<String>,
    // Strict-mode user functions (wrapper or inline-closure symbols).
    // Each entry produces one `js_register_closure_strict_function` call so
    // call/apply/bind can apply spec OrdinaryCallBindThis (#4850).
    user_fn_wrapper_strict: &std::collections::HashSet<String>,
    // `(wrapper_symbol, display_name)` for every top-level user function
    // we want `console.log` / `util.inspect` to label with the original
    // JS name. Each entry produces one `js_register_function_name` call
    // in `__perry_init_strings_<prefix>` so the registry is populated
    // before user code runs. See #1202.
    user_fn_display_names: &[(String, String)],
    // #4101: `(wrapper_symbol, source_text)` for every user function whose
    // original source we retained. Each entry produces one
    // `js_register_function_source` call in `__perry_init_strings_<prefix>`
    // so `fn.toString()` can reconstruct the source.
    user_fn_source: &[(String, String)],
) {
    for entry in strings.iter() {
        // .rodata bytes — `[N+1 x i8]` because we include the null terminator.
        llmod.add_named_string_constant(&entry.bytes_global, entry.byte_len + 1, &entry.escaped_ir);
        llmod.add_internal_global(&entry.handle_global, DOUBLE, "0.0");
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
    // Each entry becomes one `js_register_function_name(<sym>, <str>,
    // <len>)` call inside the init function. See #1202.
    let mut user_fn_name_constants: Vec<(String, String, usize)> = Vec::new();
    for (wrapper_sym, display_name) in user_fn_display_names {
        if wrapper_sym.is_empty() || display_name.is_empty() {
            continue;
        }
        let (const_name, byte_len) = llmod.add_string_constant(display_name);
        user_fn_name_constants.push((wrapper_sym.clone(), const_name, byte_len));
    }

    // #4101: pre-allocate string constants for function-source registration,
    // mirroring the name constants above (same borrow ordering: mint the
    // rodata globals BEFORE `init_fn` claims `&mut llmod`).
    let mut user_fn_source_constants: Vec<(String, String, usize)> = Vec::new();
    for (wrapper_sym, source_text) in user_fn_source {
        if wrapper_sym.is_empty() || source_text.is_empty() {
            continue;
        }
        let (const_name, byte_len) = llmod.add_string_constant(source_text);
        user_fn_source_constants.push((wrapper_sym.clone(), const_name, byte_len));
    }

    // Pre-allocate string constants for class-name registration. We need
    // these BEFORE `init_fn` is created, because once `init_fn` borrows
    // `llmod` we can no longer mutate the module's constant pool. (#1021.)
    let mut named_class_name_constants: Vec<(u32, String, usize)> = Vec::new();
    {
        let mut named: Vec<(u32, String)> = Vec::new();
        for (class_name, class) in classes.iter() {
            if *class_name != class.name {
                continue;
            }
            let cid = match class_ids.get(class_name).copied() {
                Some(c) if c != 0 => c,
                _ => continue,
            };
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
    // ~400K-instruction basic block that `clang -O0` (forced for oversized
    // modules, #4880) is catastrophically superlinear on (~36 min). Every init
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
        format!("__perry_init_strings_{}", module_prefix),
        ops_per_chunk,
    );

    for entry in strings.iter() {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let bytes_ref = format!("@{}", entry.bytes_global);
        let handle_ref = format!("@{}", entry.handle_global);
        let len_str = entry.byte_len.to_string();
        let from_bytes_fn = if entry.is_wtf8 {
            "js_string_from_wtf8_bytes"
        } else {
            "js_string_from_bytes"
        };
        let handle = blk.call(I64, from_bytes_fn, &[(PTR, &bytes_ref), (I32, &len_str)]);
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

    // Register display names for top-level user functions so
    // `console.log(myFn)` prints `[Function: myFn]` instead of
    // `[Function (anonymous)]`. The runtime registry is keyed on the
    // wrapper's compiled address (`__perry_wrap_<name>`), which is
    // what `js_closure_alloc_singleton` stamps into ClosureHeader.
    // See #1202.
    for (wrapper_sym, name_const, name_len) in &user_fn_name_constants {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let wrapper_ref = format!("@{}", wrapper_sym);
        let name_ref = format!("@{}", name_const);
        let len_str = name_len.to_string();
        blk.call_void(
            "js_register_function_name",
            &[(PTR, &wrapper_ref), (PTR, &name_ref), (I32, &len_str)],
        );
    }

    // #4101: register each function's retained source text against the same
    // wrapper/closure address `js_closure_alloc_singleton` stamps into the
    // ClosureHeader, so `fn.toString()` resolves the source by func_ptr.
    for (wrapper_sym, source_const, source_len) in &user_fn_source_constants {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let wrapper_ref = format!("@{}", wrapper_sym);
        let source_ref = format!("@{}", source_const);
        let len_str = source_len.to_string();
        blk.call_void(
            "js_register_function_source",
            &[(PTR, &wrapper_ref), (PTR, &source_ref), (I32, &len_str)],
        );
    }

    // Build per-class keys arrays via js_build_class_keys_array,
    // store the result in the per-class keys global. Done ONCE at
    // module init; every `new ClassName()` call from then on does a
    // single global load + inline allocator call (no SHAPE_CACHE
    // lookup, no js_build_class_keys_array overhead).
    for (idx, (global_name, packed, field_count, _raw_mask_words, _pointer_mask_words)) in
        class_keys_init_data.iter().enumerate()
    {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        // Resolve class id from the global name. The global name is
        // `perry_class_keys_<modprefix>__<class>` so we strip the
        // prefix to recover the sanitized class name and look up
        // the id by walking class_ids. Since multiple classes might
        // have the same sanitized name (rare but possible), we just
        // pick the first matching one — class_ids is keyed by the
        // pre-sanitized name so a direct lookup works for ASCII.
        let prefix = format!("perry_class_keys_{}__", module_prefix);
        let sanitized_class = global_name.strip_prefix(&prefix).unwrap_or("");
        let class_id = class_ids
            .iter()
            .find(|(k, _)| sanitize(k) == sanitized_class)
            .map(|(_, &v)| v)
            .unwrap_or(0);

        let cid_str = class_id.to_string();
        let fc_str = field_count.to_string();
        let packed_ref = if packed.is_empty() {
            "null".to_string()
        } else {
            format!("@{}", packed_global_names[idx])
        };
        let len_str = packed.len().to_string();
        let arr = blk.call(
            I64,
            "js_build_class_keys_array",
            &[
                (I32, &cid_str),
                (I32, &fc_str),
                (PTR, &packed_ref),
                (I32, &len_str),
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
    }

    // Register the parent-class chain for every class with a parent.
    // The runtime allocators do this on every alloc; the inline
    // bump allocator skips it. Without this one-time call, the
    // CLASS_REGISTRY misses the `child → parent` edge and walks of
    // the inheritance chain (e.g. `instanceof Shape` on a `Square`
    // where `Square extends Rectangle extends Shape`) terminate
    // prematurely. We emit one call per inheriting class, sorted by
    // class id for deterministic ordering.
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
    // (class_id, name, llvm_symbol, total_param_count, has_synth_args, has_rest, spec_length)
    let mut method_triples: Vec<(u32, String, String, u32, bool, bool, u32)> = Vec::new();
    // #1788: (cid, static-method name, perry_static_* symbol, param_count,
    // has_rest). Registered into the runtime CLASS_STATIC_METHODS table so a
    // subclass whose parent is a class-expression value inherits the parent's
    // static methods (`class Sub extends make(...) {}; Sub.greet()`); has_rest
    // tells the dispatcher to bundle trailing args for a `...rest` param.
    let mut static_method_triples: Vec<(u32, String, String, u32, bool, u32)> = Vec::new();
    // #1787: (cid, standalone-constructor symbol, total_param_count).
    // Registered into CLASS_CONSTRUCTORS so `new <classObjectValue>()` (a
    // class-expression value constructed dynamically) can replay the class's
    // constructor + field initializers on the new instance. Only consulted by
    // the heap-class-object arm of `js_new_function_construct`, so it's
    // behavior-neutral for top-level class declarations (INT32 ref `new`).
    let mut ctor_triples: Vec<(u32, String, u32)> = Vec::new();
    // #wall3: class ctors with a rest param (`constructor(...args)`) need their
    // standalone `_constructor` func_ptr registered in CLOSURE_REST_REGISTRY so
    // a member-new (`new ns.Sub(opts)` → js_new_function_construct →
    // js_native_call_value) BUNDLES trailing args into the rest array. Without
    // this the rest param binds to the first arg as a scalar (a=opts, not
    // [opts]) and `super(...args)` spreads a bare object → 0x400000000 mis-box →
    // crash (Next.js `new c.AppPageRouteModule({...})`). Mirrors the
    // closure-rest registration but keyed by the `_constructor` symbol.
    let mut ctor_rest_regs: Vec<(String, usize)> = Vec::new();
    for (class_name, class) in classes.iter() {
        // Refs #486: skip alias keys (class_table now contains both the
        // canonical name and self-binding aliases like `_X` from
        // `var X = class _X`); the symbol emission iterates by canonical
        // class.name. Without this skip the alias key generates bogus
        // symbol names like `perry_method_<mod>___X__method` (extra
        // leading underscore from sanitize("_X")) that don't resolve at
        // link time.
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name) {
            Some(&c) if c != 0 => c,
            _ => continue,
        };
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
            let has_rest = method
                .params
                .last()
                .map(|p| p.is_rest && p.arguments_object.is_none())
                .unwrap_or(false);
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
            ));
        }
        // #1788: static methods are emitted as `perry_static_*` (no `this`
        // param). Collect them for the runtime CLASS_STATIC_METHODS table.
        for sm in &class.static_methods {
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
            ));
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
        // #wall3: record the rest-param position (in USER params) so the runtime
        // bundles trailing args at the dynamic member-new dispatch path.
        if let Some(rest_idx) = class
            .constructor
            .as_ref()
            .and_then(|c| c.params.iter().position(|p| p.is_rest))
        {
            ctor_rest_regs.push((ctor_symbol.clone(), rest_idx));
        }
        ctor_triples.push((cid, ctor_symbol, ctor_params));
    }
    method_triples.sort_unstable();
    for (cid, method_name, llvm_name, param_count, has_synth_args, has_rest, spec_length) in
        method_triples
    {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        // The pre-intern pass before `emit_string_pool` ensured every
        // method name has a string pool entry; look it up here without
        // mutating the pool.
        let entry = match strings.iter().find(|e| e.value == method_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        // Cast the method function pointer to i64 via ptrtoint so the
        // runtime can store it as a `usize` in the VTABLE_REGISTRY
        // entry. The `inttoptr` round-trip in `call_vtable_method`
        // restores it for the indirect call.
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        let has_synth_args_str = if has_synth_args { "1" } else { "0" };
        let has_rest_str = if has_rest { "1" } else { "0" };
        blk.call_void(
            "js_register_class_method",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
                (I64, &param_count.to_string()),
                (I64, has_synth_args_str),
                (I64, has_rest_str),
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
    for (cid, method_name, llvm_name, param_count, has_rest, spec_length) in static_method_triples {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.iter().find(|e| e.value == method_name) {
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
    }
    // #1787: register each class's standalone constructor into
    // CLASS_CONSTRUCTORS. ptrtoint @symbol both stores the function pointer
    // and keeps the constructor alive past dead-code elimination.
    ctor_triples.sort_unstable();
    for (cid, ctor_symbol, ctor_params) in ctor_triples {
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
            ],
        );
    }
    // #wall3: register rest-bearing class ctors' func_ptrs in the closure-rest
    // side table so the dynamic member-new dispatch (js_native_call_value via
    // js_new_function_construct) bundles trailing args into the rest array,
    // matching the static `new` path. See `ctor_rest_regs` above.
    ctor_rest_regs.sort_unstable();
    for (ctor_symbol, rest_idx) in ctor_rest_regs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", ctor_symbol);
        blk.call_void(
            "js_register_closure_rest",
            &[(PTR, &func_ref), (I32, &rest_idx.to_string())],
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
        let mut anon_shape_ids: Vec<u32> = Vec::new();
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
            if class_name.starts_with("__AnonShape_") {
                anon_shape_ids.push(cid);
            } else {
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
        anon_shape_ids.sort_unstable();
        anon_shape_ids.dedup();
        for cid in anon_shape_ids {
            chunker.roll_if_full();
            let blk = chunker.current_block();
            blk.call_void(
                "js_register_anon_shape_class_id",
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
    let mut getter_pairs: Vec<(u32, String, String, bool)> = Vec::new();
    for (class_name, class) in classes.iter() {
        // Refs #486: skip alias keys (see method-emission loop above).
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
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
            getter_pairs.push((cid, prop.clone(), llvm_name, is_static));
        }
    }
    getter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name, is_static) in getter_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.iter().find(|e| e.value == prop_name) {
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
    let mut setter_pairs: Vec<(u32, String, String, bool, u32)> = Vec::new();
    for (class_name, class) in classes.iter() {
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
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
            setter_pairs.push((cid, prop.clone(), llvm_name, is_static, spec_length));
        }
    }
    setter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name, is_static, spec_length) in setter_pairs {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let entry = match strings.iter().find(|e| e.value == prop_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        // Register the setter's spec `.length` keyed by its func_ptr so
        // `Object.getOwnPropertyDescriptor(proto, prop).set.length` reports
        // the default-aware count instead of the raw ABI arity. Static
        // accessors are emitted under a no-`this` `perry_static_…` symbol, so
        // the same func_ptr is what a value-read of `.set` binds.
        blk.call_void(
            "js_register_closure_length",
            &[(PTR, &func_ref), (I32, &spec_length.to_string())],
        );
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
            ],
        );
    }

    // Issue #493: register each rest-bearing closure body's func_ptr ->
    // fixed_arity in the runtime's closure-rest side table. `js_closure_callN`
    // consults it to bundle trailing args at call sites where codegen
    // doesn't know the closure's arity statically (e.g. `obj.cb(a, b, c)`
    // where `cb` is a class field holding `(...args) => …`). Without this
    // entry the closure body sees the first arg as the rest param itself,
    // not the bundled array — `args.length` reads `1` against the string
    // value, and trailing args are dropped. Static call sites (named fns,
    // `Expr::FuncRef`, `let f = (...args)=>…; f(a,b,c)`) keep their
    // existing call-site bundling and never enter this dispatch path.
    let mut sorted_rest: Vec<(u32, usize)> = closure_rest_params
        .iter()
        .map(|(fid, ri)| (*fid, *ri))
        .collect();
    sorted_rest.sort_unstable();
    for (fid, fixed_arity) in sorted_rest {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        // Refs #915 (gap 1 from #899): closures whose rest param is the
        // synthesized `arguments` use the synthetic-arguments registration
        // so the runtime bundles ALL args into the rest slot.
        let runtime_fn = if closure_rest_and_arguments.contains(&fid) {
            "js_register_closure_rest_and_arguments"
        } else if closure_synthetic_arguments.contains(&fid) {
            "js_register_closure_synthetic_arguments"
        } else {
            "js_register_closure_rest"
        };
        blk.call_void(
            runtime_fn,
            &[(PTR, &func_ref), (I32, &fixed_arity.to_string())],
        );
    }

    // Refs #421: register every non-rest closure's declared param count so
    // `js_native_call_value` can pad missing trailing args with TAG_UNDEFINED
    // when a closure stored as a class field is invoked method-style on an
    // any-typed receiver with fewer args than declared. Rest-bearing closures
    // are already handled by the closure-rest registry above (which pads
    // internally via `dispatch_rest_bundled`).
    let mut sorted_arities: Vec<(u32, u32)> = closure_arities
        .iter()
        .map(|(fid, arity)| (*fid, *arity))
        .collect();
    sorted_arities.sort_unstable();
    for (fid, arity) in sorted_arities {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        blk.call_void(
            "js_register_closure_arity",
            &[(PTR, &func_ref), (I32, &arity.to_string())],
        );
    }

    // Register ECMAScript-visible `.length` for all closures. This is
    // intentionally separate from declared arity: default parameters lower
    // into body prologues, so dispatch still needs the full ABI arity, while
    // `fn.length` stops at the first default/rest parameter.
    let mut sorted_lengths: Vec<(u32, u32)> = closure_lengths
        .iter()
        .map(|(fid, length)| (*fid, *length))
        .collect();
    sorted_lengths.sort_unstable();
    for (fid, length) in sorted_lengths {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        blk.call_void(
            "js_register_closure_length",
            &[(PTR, &func_ref), (I32, &length.to_string())],
        );
    }

    let mut sorted_arrows: Vec<u32> = closure_arrow_functions.iter().copied().collect();
    sorted_arrows.sort_unstable();
    for fid in sorted_arrows {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        blk.call_void("js_register_closure_arrow_function", &[(PTR, &func_ref)]);
    }

    // Issue #653: register `__perry_wrap_<name>` wrappers for top-level user
    // functions whose source signature includes a rest param. Mirrors the
    // closure-rest loop above but keyed on the wrapper's symbol rather than
    // the closure body. See `user_fn_wrapper_rest` doc on this fn's signature.
    let mut sorted_wrappers: Vec<(String, usize)> = user_fn_wrapper_rest.to_vec();
    sorted_wrappers.sort();
    let rest_wrapper_names: std::collections::HashSet<String> =
        sorted_wrappers.iter().map(|(s, _)| s.clone()).collect();
    for (wrap_sym, fixed_arity) in sorted_wrappers {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        // Refs #915 (gap 1 from #899): wrappers whose underlying function
        // declared a synthesized `arguments` rest param need the
        // synthetic-arguments registration.
        let runtime_fn = if user_fn_wrapper_rest_and_arguments.contains(&wrap_sym) {
            "js_register_closure_rest_and_arguments"
        } else if user_fn_wrapper_synthetic_arguments.contains(&wrap_sym) {
            "js_register_closure_synthetic_arguments"
        } else {
            "js_register_closure_rest"
        };
        blk.call_void(
            runtime_fn,
            &[(PTR, &func_ref), (I32, &fixed_arity.to_string())],
        );
    }

    // Register declared ABI param count for `__perry_wrap_<name>` wrappers of
    // every non-rest top-level user function. Mirrors the closure-arity loop
    // above; `.length` is registered separately below.
    let mut sorted_wrapper_arities: Vec<(String, u32)> = user_fn_wrapper_arity
        .iter()
        .filter(|(name, _)| !rest_wrapper_names.contains(name))
        .cloned()
        .collect();
    sorted_wrapper_arities.sort();
    for (wrap_sym, arity) in sorted_wrapper_arities {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void(
            "js_register_closure_arity",
            &[(PTR, &func_ref), (I32, &arity.to_string())],
        );
    }

    // Register spec `.length` for all top-level user-function wrappers,
    // including rest wrappers. Ramda's `converge` / `juxt` / `useWith`
    // chains read `fn.length` from function values to compute curry arities.
    let mut sorted_wrapper_lengths: Vec<(String, u32)> = user_fn_wrapper_length.to_vec();
    sorted_wrapper_lengths.sort();
    for (wrap_sym, length) in sorted_wrapper_lengths {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void(
            "js_register_closure_length",
            &[(PTR, &func_ref), (I32, &length.to_string())],
        );
    }

    let mut sorted_async_wrappers: Vec<String> = user_fn_wrapper_async.iter().cloned().collect();
    sorted_async_wrappers.sort();
    for wrap_sym in sorted_async_wrappers {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void("js_register_closure_async_function", &[(PTR, &func_ref)]);
    }

    let mut sorted_generator_wrappers: Vec<String> =
        user_fn_wrapper_generator.iter().cloned().collect();
    sorted_generator_wrappers.sort();
    for wrap_sym in sorted_generator_wrappers {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void(
            "js_register_closure_generator_function",
            &[(PTR, &func_ref)],
        );
    }

    // #3664: async-generator wrappers. These are ALSO in
    // `user_fn_wrapper_generator` above (they share the sync generator
    // lowering); this extra registration is what lets the runtime tell an
    // `async function*` apart from a `function*`.
    let mut sorted_async_generator_wrappers: Vec<String> =
        user_fn_wrapper_async_generator.iter().cloned().collect();
    sorted_async_generator_wrappers.sort();
    for wrap_sym in sorted_async_generator_wrappers {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void(
            "js_register_closure_async_generator_function",
            &[(PTR, &func_ref)],
        );
    }

    let mut sorted_strict_wrappers: Vec<String> = user_fn_wrapper_strict.iter().cloned().collect();
    sorted_strict_wrappers.sort();
    for wrap_sym in sorted_strict_wrappers {
        chunker.roll_if_full();
        let blk = chunker.current_block();
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void("js_register_closure_strict_function", &[(PTR, &func_ref)]);
    }

    let chunk_names = chunker.finish();
    let init_name = format!("__perry_init_strings_{}", module_prefix);
    let init_fn = llmod.define_function(&init_name, VOID, vec![]);
    let _ = init_fn.create_block("entry");
    let blk = init_fn.block_mut(0).unwrap();
    for cname in &chunk_names {
        blk.call_void(cname, &[]);
    }
    blk.ret_void();
}
