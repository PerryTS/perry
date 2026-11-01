//! Maintained C payload layouts and helpers own canonical read storage.
use super::*;
use wit_parser::Type;

impl Generator<'_> {
    pub(super) fn reader_adapter(&mut self, ty: u32, id: TypeId, name: &str) -> Result<()> {
        let (stream, payload) = match self.resolve.types[id].kind {
            TypeDefKind::Future(payload) => (false, payload),
            TypeDefKind::Stream(payload) => (true, payload),
            _ => unreachable!("endpoint operation type"),
        };
        let world = self.resolve.worlds[self.id].name.to_snake_case();
        let upper = world.to_uppercase();
        let stem = format!("perry_read_{ty}");
        let helper = name.strip_suffix("_t").unwrap();
        let mut emitter = values::Emitter::new(
            self.resolve,
            &self.names,
            &self.resources,
            &self.endpoints,
            &world,
        );
        writeln!(
            emitter.source,
            "typedef struct {{ perry_io base; size_t capacity; {} }} {stem}_state;",
            payload
                .map(|ty| format!("{} *items;", emitter.ctype(ty)))
                .unwrap_or_default()
        )?;
        writeln!(emitter.source, "static void {stem}_deliver(void *pointer) {{\n{stem}_state *data = pointer;\nperry_io *io = &data->base;\nvoid *f = io->frame;\nperry_require(!io->complete && !io->joined);\nio->complete = true;\n{world}_waitable_state_t state = {upper}_WAITABLE_STATE(io->status);\nuint32_t count = {upper}_WAITABLE_COUNT(io->status);\nperry_require(state == {upper}_WAITABLE_COMPLETED || state == {upper}_WAITABLE_DROPPED || state == {upper}_WAITABLE_CANCELLED);\nperry_require(count <= data->capacity);\nbool terminal = state == {upper}_WAITABLE_DROPPED{};\nbool abort = io->cancelled || state == {upper}_WAITABLE_CANCELLED{};\nuint32_t result = 0;\nint32_t observed = abort ? 0 : perry_component_frame_endpoint_prepare(f);\nperry_require(observed >= 0);\nif (observed) {{\nio->claimed = true;", if stream { "" } else { " || count == 1" }, if stream { "" } else { " || count == 0" })?;
        if stream {
            let chunk = if payload.is_some_and(|ty| is_byte(self.resolve, ty)) {
                emitter.slot("perry_component_frame_bytes(f, (const uint8_t *)data->items, count)")
            } else {
                let chunk = emitter.slot("perry_component_frame_array(f, count)");
                emitter
                    .source
                    .push_str("for (uint32_t index = 0; index < count; ++index) {\n");
                let value = if let Some(payload) = payload {
                    emitter.lift(payload, "data->items[index]")?
                } else {
                    let empty = emitter.slot("perry_component_frame_array(f, 0)");
                    emitter.slot(&format!("perry_component_frame_array_get(f, {empty}, 0)"))
                };
                writeln!(
                    emitter.source,
                    "perry_component_frame_array_set(f, {chunk}, index, {value});\n}}"
                )?;
                chunk
            };
            emitter.source.push_str("result = perry_component_frame_object(f);\nuint32_t done = perry_component_frame_bool(f, terminal && count == 0);\nperry_component_frame_set(f, result, (const uint8_t *)\"done\", 4, done);\nif (!(terminal && count == 0)) {\n");
            writeln!(
                emitter.source,
                "perry_component_frame_set(f, result, (const uint8_t *)\"value\", 5, {chunk});\n}}"
            )?;
        } else {
            emitter.source.push_str("perry_require(count == 1);\n");
            let value = if let Some(payload) = payload {
                emitter.lift(payload, "data->items[0]")?
            } else {
                let empty = emitter.slot("perry_component_frame_array(f, 0)");
                emitter.slot(&format!("perry_component_frame_array_get(f, {empty}, 0)"))
            };
            writeln!(emitter.source, "result = {value};")?;
        }
        emitter.source.push_str("perry_require(!perry_component_frame_failed(f));\n} else { abort = true; }\nperry_require(perry_component_frame_endpoint_finish(f, result, (terminal ? 1u : 0u) | (abort ? 2u : 0u)) == 0);\n}\n");
        writeln!(emitter.source, "static void {stem}_cancel(perry_io *io) {{\nperry_require(!io->cancelled && !io->complete);\nio->cancelled = true;\nif (io->joined) {{ {world}_waitable_join(io->reader, 0); io->joined = false; }}\nio->status = {helper}_cancel_read(io->reader);\nperry_require(io->status != {upper}_WAITABLE_STATUS_BLOCKED);\n}}")?;
        writeln!(emitter.source, "static void {stem}_destroy(perry_io *io) {{\n{stem}_state *data = ({stem}_state *)io;\nperry_require(io->complete && !io->joined);\nuint32_t count = {upper}_WAITABLE_COUNT(io->status);\nperry_require(count <= data->capacity);\n(void)count;")?;
        if let Some(payload) = payload {
            emitter.source.push_str(
                "for (uint32_t index = 0; index < count; ++index) {\nif (!io->claimed) {\n",
            );
            emitter.discard_owned(payload, "data->items[index]");
            emitter.source.push_str("}\n");
            emitter.free(payload, "&data->items[index]");
            emitter.source.push_str("}\nfree(data->items);\n");
        }
        emitter
            .source
            .push_str("perry_component_frame_drop(io->frame);\nfree(data);\n}\n");
        writeln!(emitter.source, "static int32_t {stem}_start(void *f, uint32_t reader, uint32_t capacity) {{\nperry_task *parent = perry_active_task;\nif (!parent || !parent->asynchronous || parent->cancelled) {{ perry_bad(f, \"Guest endpoint I/O requires an async export task\"); return -1; }}\nif (capacity == 0 || capacity > 65536{}) {{ perry_bad(f, \"Invalid guest endpoint read capacity\"); return -1; }}\n{stem}_state *data = calloc(1, sizeof(*data));\nif (!data) {{ perry_bad(f, \"Guest read allocation failed\"); return -1; }}\ndata->capacity = capacity;", if stream { "" } else { " || capacity != 1" })?;
        if payload.is_some() {
            emitter.source.push_str("data->items = calloc(capacity, sizeof(*data->items));\nif (!data->items) { free(data); perry_bad(f, \"Guest read buffer allocation failed\"); return -1; }\n");
        }
        writeln!(emitter.source, "perry_io *io = &data->base;\nio->frame = f; io->reader = reader; io->type = {ty};\nio->event = {upper}_EVENT_{}_READ;\nio->deliver = {stem}_deliver; io->cancel = {stem}_cancel; io->destroy = {stem}_destroy;\nio->next = parent->io; parent->io = io;", if stream { "STREAM" } else { "FUTURE" })?;
        let mut args = vec!["reader"];
        if payload.is_some() {
            args.push("data->items");
        }
        if stream {
            args.push("capacity");
        }
        writeln!(emitter.source, "io->status = {helper}_read({});\nif (io->status == {upper}_WAITABLE_STATUS_BLOCKED) {{\nperry_task_set(parent);\n{world}_waitable_join(reader, parent->set);\nio->joined = true;\n}} else {{\n{stem}_deliver(io);\n}}\nreturn 0;\n}}", args.join(", "))?;
        self.source.push_str(&emitter.source);
        Ok(())
    }
}

fn is_byte(resolve: &Resolve, ty: Type) -> bool {
    match ty {
        Type::U8 => true,
        Type::Id(id) => match resolve.types[id].kind {
            TypeDefKind::Type(ty) => is_byte(resolve, ty),
            _ => false,
        },
        _ => false,
    }
}
