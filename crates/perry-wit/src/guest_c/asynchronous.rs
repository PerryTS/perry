//! Experimental generated async functions. The public CLI remains fail-closed.
mod imports;
#[cfg(test)]
mod tests;
use super::*;

pub(super) fn add_wake_anchor(resolve: &mut Resolve, id: WorldId) -> Result<String> {
    let mut n = 0;
    let name = loop {
        let candidate = format!("perry-scheduler-signal-{n}");
        if !resolve.worlds[id]
            .exports
            .contains_key(&WorldKey::Name(candidate.clone()))
            && !resolve.worlds[id]
                .imports
                .contains_key(&WorldKey::Name(candidate.clone()))
            && !resolve
                .types
                .iter()
                .any(|(_, ty)| ty.name.as_deref() == Some(&candidate))
        {
            break candidate;
        }
        n += 1;
    };
    // Append after converting the public world: existing nominal/resource IDs,
    // function indices and the serialized contract stay unchanged.
    let signal = resolve.types.alloc(wit_parser::TypeDef {
        // A private name keeps its helper symbols distinct from public unit
        // streams, including unit streams owned by imported interfaces.
        name: Some(name.clone()),
        kind: TypeDefKind::Stream(None),
        owner: wit_parser::TypeOwner::None,
        docs: Default::default(),
        stability: wit_parser::Stability::Unknown,
        span: Default::default(),
        external_id: None,
    });
    let function = Function {
        name: name.clone(),
        kind: wit_parser::FunctionKind::Freestanding,
        params: Vec::new(),
        result: Some(wit_parser::Type::Id(signal)),
        docs: Default::default(),
        stability: wit_parser::Stability::Unknown,
        span: Default::default(),
        external_id: None,
    };
    resolve.worlds[id]
        .exports
        .insert(WorldKey::Name(name.clone()), WorldItem::Function(function));
    Ok(name)
}

pub(super) fn scheduler(world: &str, anchor: &str) -> String {
    include_str!("scheduler.h")
        .replace("$W", world)
        .replace("$S", &format!("{world}_{}", anchor.to_snake_case()))
        .replace("$U", &world.to_uppercase())
        .replace("$ANCHOR", &anchor.to_snake_case())
}

impl Generator<'_> {
    pub(super) fn experimental_export(
        &mut self,
        function: &Function,
        interface: usize,
        index: usize,
        cname: &str,
    ) -> Result<()> {
        let snake = self.resolve.worlds[self.id].name.to_snake_case();
        let upper = snake.to_uppercase();
        let stem = format!("perry_result_{interface}_{index}");
        let async_ = function.kind.is_async();
        let mut emitter = values::Emitter::new(
            self.resolve,
            &self.names,
            &self.resources,
            &self.endpoints,
            &snake,
        );
        let result_ty = function.result.map(|ty| emitter.ctype(ty));
        writeln!(
            emitter.source,
            "typedef struct {{ void *frame; uint32_t value; perry_allocations arena; {} }} {stem};",
            result_ty
                .as_ref()
                .map(|ty| format!("{ty} result;"))
                .unwrap_or_default()
        )?;
        writeln!(emitter.source, "static void {stem}_lower(void *pointer) {{\n{stem} *data = pointer;\nvoid *f = data->frame;\nperry_allocations arena = data->arena;\n(void)f;")?;
        if let Some(ty) = function.result {
            emitter.lower(ty, "data->value", "data->result")?;
        }
        emitter.source.push_str("data->arena = arena;\n}\n");
        writeln!(emitter.source, "static {stem} {stem}_convert(perry_task *t, uint32_t value) {{\n{stem} data = {{0}};\ndata.frame = perry_component_frame_new();\ndata.value = perry_component_frame_copy(data.frame, t->frame, value);\nint32_t status = perry_task_resume(t, {stem}_lower, &data);\nif (status < 0 || perry_component_frame_failed(data.frame)) {{\nperry_allocations_end(&data.arena, false);\nperry_component_frame_drop(data.frame);\n__builtin_trap();\n}}\nreturn data;\n}}")?;
        if async_ {
            writeln!(emitter.source, "static {snake}_callback_code_t {stem}_step(perry_task *t) {{\nif (!t->returned) {{\nuint32_t value = 0;\nif (!perry_task_poll(t, &value)) return perry_task_wait(t);\n{stem} data = {stem}_convert(t, value);\nperry_require(perry_task_checkpoint(t) >= 0);\nperry_component_frame_resource_commit(data.frame);\nperry_require(!perry_component_frame_failed(data.frame));\nperry_require(perry_component_frame_resource_scope_end(t->frame) == 0);\n{cname}_return({});\nperry_allocations_end(&data.arena, false);\nperry_component_frame_drop(data.frame);\nt->returned = true;\n}} else {{\nperry_require(perry_task_checkpoint(t) >= 0);\n}}\nif (t->children) return perry_task_wait(t);\n{snake}_context_set_0(NULL);\nperry_task_drop(t);\nreturn {upper}_CALLBACK_CODE_EXIT;\n}}", if result_ty.is_some() { "data.result" } else { "" })?;
            writeln!(emitter.source, "{snake}_callback_code_t {cname}_callback({snake}_event_t *event) {{\nperry_task *t = {snake}_context_get_0();\nif (event->event == {upper}_EVENT_CANCEL) return perry_task_cancel(t);\nperry_task_event(t, event);\nreturn {stem}_step(t);\n}}")?;
        }
        let indirect = !async_
            && function
                .result
                .is_some_and(|ty| wit_bindgen_c::is_arg_by_pointer(self.resolve, &ty));
        let ret = if async_ {
            format!("{snake}_callback_code_t")
        } else if indirect {
            "void".into()
        } else {
            result_ty.clone().unwrap_or_else(|| "void".into())
        };
        let mut params: Vec<_> = function
            .params
            .iter()
            .enumerate()
            .map(|(n, param)| {
                format!(
                    "{} {}arg{n}",
                    emitter.ctype(param.ty),
                    if wit_bindgen_c::is_arg_by_pointer(self.resolve, &param.ty) {
                        "*"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        if indirect {
            params.push(format!("{} *result", result_ty.as_ref().unwrap()));
        }
        if params.is_empty() {
            params.push("void".into());
        }
        writeln!(
            emitter.source,
            "{ret} {cname}({}) {{\nperry_task *t = perry_task_new({async_});\nvoid *f = t->frame;",
            params.join(", ")
        )?;
        if async_ {
            writeln!(emitter.source, "{snake}_context_set_0(t);")?;
        }
        let args = emitter.slot(&format!(
            "perry_component_frame_array(f, {})",
            function.params.len()
        ));
        for (n, param) in function.params.iter().enumerate() {
            let indirect = wit_bindgen_c::is_arg_by_pointer(self.resolve, &param.ty);
            let value = emitter.lift(
                param.ty,
                &format!("{}arg{n}", if indirect { "*" } else { "" }),
            )?;
            writeln!(
                emitter.source,
                "perry_component_frame_array_set(f, {args}, {n}, {value});"
            )?;
            emitter.free(
                param.ty,
                &format!("{}arg{n}", if indirect { "" } else { "&" }),
            );
        }
        let value = emitter.slot(&format!("perry_task_call(t, {interface}, {index}, {args})"));
        emitter
            .source
            .push_str("perry_require(!perry_component_frame_failed(f));\n");
        writeln!(
            emitter.source,
            "perry_require(perry_component_frame_task_release_arguments(f, {value}) == 0);"
        )?;
        if async_ {
            writeln!(
                emitter.source,
                "t->promise = {value};\nreturn {stem}_step(t);\n}}"
            )?;
        } else {
            writeln!(emitter.source, "perry_require(perry_task_checkpoint(t) >= 0);\n{stem} data = {stem}_convert(t, {value});\nperry_require(perry_task_checkpoint(t) >= 0);\nperry_component_frame_resource_commit(data.frame);\nperry_require(!perry_component_frame_failed(data.frame));")?;
            if indirect {
                emitter.source.push_str("*result = data.result;\n");
            }
            // Sync canonical post-return owns these result allocations.
            emitter.source.push_str("perry_allocations_end(&data.arena, true);\nperry_component_frame_drop(data.frame);\nperry_task_drop(t);\n");
            if result_ty.is_some() && !indirect {
                emitter.source.push_str("return data.result;\n");
            }
            emitter.source.push_str("}\n");
        }
        self.source.push_str(&emitter.source);
        Ok(())
    }
}
