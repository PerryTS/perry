//! Runtime selection among the entries discovered by a Worker path helper.
use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::nanbox::{double_literal, POINTER_TAG_TOP16_I64};
use crate::rooting::with_rooted_group;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

pub(super) fn lower_candidates(
    ctx: &mut FnCtx<'_>,
    paths: &[String],
    filename: &Expr,
    options: Option<&Expr>,
) -> Result<String> {
    let targets: Vec<_> = paths
        .iter()
        .filter_map(|path| ctx.dynamic_import_path_to_prefix.get(path).cloned())
        .collect();
    if targets
        .iter()
        .any(|target| target.starts_with("__node_submod__") || target.starts_with("__native_mod__"))
    {
        bail!("worker_threads Worker target must be a compiled source file");
    }
    // The driver includes lexical absolute paths for URL values as well as
    // import spellings. Sort to keep emitted IR stable.
    let mut aliases: Vec<_> = ctx
        .dynamic_import_path_to_prefix
        .iter()
        .filter(|(_, target)| targets.contains(target))
        .map(|(path, target)| (path.clone(), target.clone()))
        .collect();
    aliases.sort();
    with_rooted_group(ctx, 2, |ctx, roots| {
        let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let file = roots.lower(ctx, filename, true)?;
        if ctx.block().is_terminated() {
            return Ok(undefined);
        }
        let opts = roots.lower(ctx, options.unwrap_or(&Expr::Undefined), true)?;
        if ctx.block().is_terminated() {
            return Ok(undefined);
        }
        let file = roots.reread(ctx, file)?;
        // The path grammar proves each value is a string or URL. Decode URL
        // objects through fileURLToPath, so both encoded and unescaped hrefs
        // (including spaces) compare as filesystem paths, without coercion.
        let bits = ctx.block().bitcast_double_to_i64(&file);
        let tag = ctx.block().lshr(I64, &bits, "48");
        let is_url = ctx.block().icmp_eq(I64, &tag, POINTER_TAG_TOP16_I64);
        // #10463: both join slots are entry-block allocas, so a `new Worker`
        // inside a loop does not grow the stack per iteration.
        let normalized = ctx.func.alloca_entry(DOUBLE);
        let url_block = ctx.new_block("worker_url");
        let string_block = ctx.new_block("worker_string");
        let dispatch = ctx.new_block("worker_dispatch");
        let url_label = ctx.block_label(url_block);
        let string_label = ctx.block_label(string_block);
        let dispatch_label = ctx.block_label(dispatch);
        ctx.block().cond_br(&is_url, &url_label, &string_label);
        ctx.current_block = url_block;
        let path = ctx.block().call(
            DOUBLE,
            "js_url_file_url_to_path",
            &[(DOUBLE, &file), (DOUBLE, &undefined)],
        );
        ctx.block().store(DOUBLE, &path, &normalized);
        ctx.block().br(&dispatch_label);
        ctx.current_block = string_block;
        ctx.block().store(DOUBLE, &file, &normalized);
        ctx.block().br(&dispatch_label);
        ctx.current_block = dispatch;
        let file = ctx.block().load(DOUBLE, &normalized);
        let spec = ctx
            .block()
            .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &file)]);
        // Comparisons below do not allocate. Only the selected spawn can
        // collect, after the last use of `spec`; options remain rooted.
        let result = ctx.func.alloca_entry(DOUBLE);
        let join = ctx.new_block("worker_join");
        for (path, target) in &aliases {
            let key = ctx.strings.intern(path);
            let global = format!("@{}", ctx.strings.entry(key).handle_global);
            let key = ctx.block().load(DOUBLE, &global);
            let key = ctx
                .block()
                .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &key)]);
            let eq = ctx
                .block()
                .call(I32, "js_string_equals", &[(I64, &spec), (I64, &key)]);
            let matches = ctx.block().icmp_ne(I32, &eq, "0");
            let matched = ctx.new_block("worker_match");
            let next = ctx.new_block("worker_next");
            let matched_label = ctx.block_label(matched);
            let next_label = ctx.block_label(next);
            ctx.block().cond_br(&matches, &matched_label, &next_label);
            ctx.current_block = matched;
            // #10399: the SECOND worker-entry emitter (the multi-path
            // `new Worker(<resolved specifier>)` form; the single-path one
            // lives in `dyn_extern_i18n.rs`). Both must agree.
            //
            // `<target>__init` is the guarded wrapper: it initializes the
            // module's DEPENDENCIES and then runs the body. With a Worker in
            // the program the `__perry_init_done_*` guard is thread-local, so
            // the wrapper runs once per thread and is the only thing that
            // initializes the worker entry's imports. Entering the bare body
            // left every imported module uninitialized on that thread — in
            // OpenCode's TUI worker, `cli/heap.ts`'s string pool was still
            // empty, so `Flag.OPENCODE_AUTO_HEAP_SNAPSHOT` read a property
            // whose NAME was the empty string and threw
            // "Cannot read properties of undefined (reading '')".
            let init = if crate::codegen::program_has_worker() {
                format!("{target}__init")
            } else {
                // Process-wide guard: the wrapper would no-op after the first
                // worker, so the bare body is still the only way each worker
                // runs its own entry.
                format!("{target}__init_body")
            };
            ctx.pending_declares.push((init.clone(), VOID, vec![]));
            let entry = ctx.block().ptrtoint(&format!("@{init}"), I64);
            let options = roots.reread(ctx, opts)?;
            let worker = ctx.block().call(
                DOUBLE,
                "js_worker_threads_worker_new",
                &[(I64, &entry), (DOUBLE, &options)],
            );
            let join_label = ctx.block_label(join);
            ctx.block().store(DOUBLE, &worker, &result);
            ctx.block().br(&join_label);
            ctx.current_block = next;
        }
        let message = "worker_threads Worker filename did not match an existing compile-time-resolved worker entry";
        let message_id = ctx.strings.intern(message);
        let entry = ctx.strings.entry(message_id);
        let global = format!("@{}", entry.bytes_global);
        let len = entry.byte_len.to_string();
        ctx.block().call_void(
            "js_throw_error_with_code",
            &[
                (PTR, &global),
                (I64, &len),
                (PTR, "null"),
                (I64, "0"),
                (I32, "0"),
            ],
        );
        ctx.block().unreachable();
        ctx.current_block = join;
        Ok(ctx.block().load(DOUBLE, &result))
    })
}
