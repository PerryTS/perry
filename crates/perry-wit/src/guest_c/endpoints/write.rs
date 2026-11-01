//! Complete typed batches transfer custody before canonical writes begin.
use super::*;

impl Generator<'_> {
    pub(in super::super) fn writer_adapter(
        &mut self,
        ty: u32,
        id: TypeId,
        name: &str,
    ) -> Result<()> {
        let (stream, payload) = match self.resolve.types[id].kind {
            TypeDefKind::Future(payload) => (false, payload),
            TypeDefKind::Stream(payload) => (true, payload),
            _ => unreachable!("endpoint producer type"),
        };
        let world = self.resolve.worlds[self.id].name.to_snake_case();
        let upper = world.to_uppercase();
        let helper = name.strip_suffix("_t").unwrap();
        let bytes = stream && payload.is_some_and(|ty| super::read::is_byte(self.resolve, ty));
        let mut emitter = values::Emitter::new(
            self.resolve,
            &self.names,
            &self.resources,
            &self.endpoints,
            &world,
        );
        let items = payload
            .map(|ty| format!("{} *items;", emitter.ctype(ty)))
            .unwrap_or_default();
        let remainder = if bytes {
            emitter.slot("perry_component_frame_bytes(f, data->items + count, remaining)")
        } else {
            let list = emitter.slot("perry_component_frame_array(f, remaining)");
            emitter
                .source
                .push_str("for (uint32_t index = 0; index < remaining; ++index) {\n");
            let item = if let Some(payload) = payload {
                emitter.lift(payload, "data->items[count + index]")?
            } else {
                let empty = emitter.slot("perry_component_frame_array(f, 0)");
                emitter.slot(&format!("perry_component_frame_array_get(f, {empty}, 0)"))
            };
            writeln!(
                emitter.source,
                "perry_component_frame_array_set(f, {list}, index, {item});\n}}"
            )?;
            list
        };
        let lift = std::mem::take(&mut emitter.source);
        if let Some(payload) = payload {
            emitter
                .source
                .push_str("for (uint32_t index = data->sent; index < data->length; ++index) {\n");
            emitter.discard_owned(payload, "data->items[index]");
            emitter.source.push_str("}\n");
        }
        let discard = std::mem::take(&mut emitter.source);
        if let Some(payload) = payload {
            emitter.source.push_str(
                "data->items = perry_allocate(f, &arena, data->length, sizeof(*data->items));\n",
            );
            if bytes {
                emitter.source.push_str("if (!perry_component_frame_failed(f)) perry_component_frame_bytes_copy(f, input, data->items, data->length);\n");
            } else {
                emitter.source.push_str("for (uint32_t index = 0; index < data->length && !perry_component_frame_failed(f); ++index) {\n");
                let value = if stream {
                    emitter.slot("perry_component_frame_array_get(f, input, index)")
                } else {
                    "input".into()
                };
                emitter.lower(payload, &value, "data->items[index]")?;
                emitter.source.push_str("}\n");
            }
        }
        let lower = std::mem::take(&mut emitter.source);
        let mut args = vec!["handle"];
        if payload.is_some() {
            args.push("data->items");
        }
        if stream {
            args.push("data->length");
        }
        let mut source = include_str!("write.h").to_string();
        for (key, value) in [
            ("$ITEMS", items),
            ("$STEM", format!("perry_write_{ty}")),
            ("$TYPE", ty.to_string()),
            ("$HELPER", helper.to_string()),
            ("$CTYPE", name.to_string()),
            ("$STREAM", stream.to_string()),
            (
                "$EVENT",
                if stream { "STREAM" } else { "FUTURE" }.to_string(),
            ),
            (
                "$LENGTH",
                if stream {
                    if bytes {
                        "perry_component_frame_bytes_len(f, input)"
                    } else {
                        "perry_component_frame_array_len(f, input)"
                    }
                } else {
                    "1"
                }
                .to_string(),
            ),
            (
                "$COUNT",
                if stream {
                    format!("{upper}_WAITABLE_COUNT(io->status)")
                } else {
                    format!("(io->status == {upper}_WAITABLE_COMPLETED ? 1u : 0u)")
                },
            ),
            (
                "$VALID",
                if stream {
                    format!("state == {upper}_WAITABLE_COMPLETED || state == {upper}_WAITABLE_DROPPED || state == {upper}_WAITABLE_CANCELLED")
                } else {
                    format!("io->status == (uint32_t)state && (state == {upper}_WAITABLE_COMPLETED || state == {upper}_WAITABLE_DROPPED || state == {upper}_WAITABLE_CANCELLED)")
                },
            ),
            (
                "$ENDED",
                if stream { "" } else { "|| count == 1" }.to_string(),
            ),
            (
                "$WRITTEN",
                if stream { "false" } else { "count == 1" }.to_string(),
            ),
            ("$LIFT_REMAINDER", lift),
            ("$REMAINDER_SLOT", remainder),
            ("$DISCARD_REMAINDER", discard),
            ("$LOWER_INPUT", lower),
            (
                "$NATIVE_WRITE",
                format!("{helper}_write({})", args.join(", ")),
            ),
            ("$W", world),
            ("$U", upper),
        ] {
            source = source.replace(key, &value);
        }
        self.source.push_str(&source);
        Ok(())
    }
}
