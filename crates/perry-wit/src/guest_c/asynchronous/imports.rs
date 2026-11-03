//! Typed native areas remain alive until a canonical subtask returns. The
//! maintained C backend supplies their signatures and layout; JS stays rooted.
use super::*;
use wit_parser::abi::AbiVariant;

impl Generator<'_> {
    pub(in super::super) fn experimental_import(
        &mut self,
        function: &Function,
        interface: usize,
        index: usize,
        cname: &str,
    ) -> Result<()> {
        let snake = self.resolve.worlds[self.id].name.to_snake_case();
        let stem = format!("perry_import_{interface}_{index}");
        let mut emitter = values::Emitter::new(self.resolve, &self.names, &self.resources, &snake);
        let indirect = self
            .resolve
            .wasm_signature(AbiVariant::GuestImportAsync, function)
            .indirect_params;
        let aggregate = indirect && function.params.len() > 1;
        writeln!(
            emitter.source,
            "typedef struct {{\nperry_child base;\nperry_allocations arena;"
        )?;
        if aggregate {
            writeln!(emitter.source, "{cname}_args_t args;")?;
        } else {
            for (n, param) in function.params.iter().enumerate() {
                writeln!(emitter.source, "{} arg{n};", emitter.ctype(param.ty))?;
            }
        }
        if let Some(ty) = function.result {
            writeln!(emitter.source, "{} result;", emitter.ctype(ty))?;
        }
        writeln!(emitter.source, "}} {stem}_state;\nstatic void {stem}_finish(void *pointer) {{\n{stem}_state *data = pointer;\nvoid *f = data->base.frame;")?;
        let result = if let Some(ty) = function.result {
            emitter.lift(ty, "data->result")?
        } else {
            let unit = emitter.slot("perry_component_frame_array(f, 0)");
            emitter.slot(&format!("perry_component_frame_array_get(f, {unit}, 0)"))
        };
        if function.result.is_some_and(|ty| emitter.owns_resources(ty)) {
            // A failed compound lift may already have adopted some handles.
            // Retire the Store even for inline completion; recovering this
            // import would need per-handle ownership rollback.
            emitter
                .source
                .push_str("perry_require(!perry_component_frame_failed(f));\n");
        }
        writeln!(emitter.source, "(void)perry_component_frame_import_resolve(f, {result});\n}}\nstatic void {stem}_destroy(perry_child *child) {{\n{stem}_state *data = ({stem}_state *)child;")?;
        emitter.source.push_str("if (!child->params_consumed) {\n");
        for (n, param) in function.params.iter().enumerate() {
            let input = if aggregate {
                format!("data->args.{}", wit_bindgen_c::to_c_ident(&param.name))
            } else {
                format!("data->arg{n}")
            };
            emitter.discard_owned(param.ty, &input);
        }
        emitter.source.push_str("}\n");
        if let Some(ty) = function.result {
            emitter.source.push_str("if (child->result_ready) {\n");
            emitter.source.push_str("if (!child->result_claimed) {\n");
            emitter.discard_owned(ty, "data->result");
            emitter.source.push_str("}\n");
            emitter.free(ty, "&data->result");
            emitter.source.push_str("}\n");
        }
        emitter.source.push_str("perry_allocations_end(&data->arena, false);\nperry_component_frame_drop(child->frame);\nfree(data);\n}\n");
        writeln!(emitter.source, "static uint32_t {stem}(void *f, uint32_t args) {{\nperry_task *parent = perry_active_task;\nif (!parent || !parent->asynchronous) {{ perry_bad(f, \"Async import requires an async export task\"); return 0; }}\n{stem}_state *data = calloc(1, sizeof(*data));\nif (!data) {{ perry_bad(f, \"Component allocation failed\"); return 0; }}\nperry_allocations arena = {{0}};\n(void)args;\ndata->base.frame = f;\ndata->base.finish = {stem}_finish;\ndata->base.destroy = {stem}_destroy;")?;
        for (n, param) in function.params.iter().enumerate() {
            let input = emitter.slot(&format!("perry_component_frame_array_get(f, args, {n})"));
            let destination = if aggregate {
                format!("data->args.{}", wit_bindgen_c::to_c_ident(&param.name))
            } else {
                format!("data->arg{n}")
            };
            emitter.lower(param.ty, &input, &destination)?;
        }
        emitter.source.push_str("data->arena = arena;\n");
        // Publish only after typed parameter allocation/conversion succeeds.
        // An error before begin leaves the frame owned by the runtime.
        emitter.source.push_str("uint32_t promise = perry_component_frame_import_begin(f);\nif (!promise) { perry_allocations_end(&data->arena, false); free(data); return 0; }\n");
        emitter
            .source
            .push_str("data->base.next = parent->children;\nparent->children = &data->base;\n");
        let mut args: Vec<String> = if aggregate {
            vec!["&data->args".into()]
        } else {
            function
                .params
                .iter()
                .enumerate()
                .map(|(n, _)| format!("{}data->arg{n}", if indirect { "&" } else { "" }))
                .collect()
        };
        if function.result.is_some() {
            args.push("&data->result".into());
        }
        writeln!(emitter.source, "{snake}_subtask_status_t status = {cname}({});\nperry_child_start(parent, &data->base, status);\nreturn promise;\n}}", args.join(", "))?;
        self.source.push_str(&emitter.source);
        Ok(())
    }
}
