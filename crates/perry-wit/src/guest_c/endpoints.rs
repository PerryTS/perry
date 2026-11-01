//! Typed canonical cleanup stays in the maintained C ABI, outside Rust scopes.
use super::*;
#[cfg(all(test, target_os = "linux"))]
mod custody_tests;
mod read;
mod write;
#[cfg(all(test, target_os = "linux"))]
mod write_tests;

impl Generator<'_> {
    pub(super) fn endpoint_adapters(&mut self) -> Result<()> {
        if self.endpoint_operations.is_empty() {
            self.source
                .push_str("static void perry_endpoint_flush(void) {}\n");
            return Ok(());
        }
        self.source.push_str("static int32_t perry_endpoint_operation(void *f, uint32_t type, uint32_t reader, uint32_t operation, uint32_t argument) {\nif (operation != 0) { perry_bad(f, \"Unsupported guest endpoint operation\"); return -1; }\nswitch (type) {\n");
        for ty in self.endpoint_operations.keys() {
            writeln!(
                self.source,
                "case {ty}: return perry_read_{ty}_start(f, reader, argument);"
            )?;
        }
        self.source.push_str("default: perry_bad(f, \"Unknown guest endpoint type\"); return -1;\n}\n}\nstatic const perry_guest_endpoint_ops perry_endpoint_ops = {1, sizeof(perry_guest_endpoint_ops), perry_endpoint_operation};\n");
        self.source.push_str(
            "static uint32_t perry_producer_create(void *f, uint32_t type) {\nswitch (type) {\n",
        );
        for ty in self.endpoint_operations.keys() {
            writeln!(self.source, "case {ty}: return perry_new_{ty}(f);")?;
        }
        self.source.push_str("default: perry_bad(f, \"Unknown guest producer type\"); return 0;\n}\n}\nstatic int32_t perry_producer_write(void *f, uint32_t type, uint32_t handle, uint32_t operation, uint32_t input) {\nswitch (type) {\n");
        for ty in self.endpoint_operations.keys() {
            writeln!(
                self.source,
                "case {ty}: return perry_write_{ty}_start(f, handle, operation, input);"
            )?;
        }
        self.source.push_str("default: perry_bad(f, \"Unknown guest producer type\"); return -1;\n}\n}\nstatic const perry_guest_producer_ops perry_producer_ops = {1, sizeof(perry_guest_producer_ops), perry_producer_create, perry_producer_write, perry_producer_notify};\n");
        self.source.push_str("static bool perry_endpoint_flushing;\nstatic void perry_endpoint_flush(void) {\nif (perry_endpoint_flushing) return;\nperry_endpoint_flushing = true;\nfor (perry_task *t = perry_tasks; t; t = t->next) perry_io_sweep(t);\nvoid *f = perry_component_frame_new();\nuint32_t type = 0, handle = 0;\nint32_t status;\nwhile ((status = perry_component_frame_endpoint_pop(f, &type, &handle)) > 0) {\nperry_io_close(type, handle);\nswitch (type) {\n");
        for (ty, name) in &self.endpoint_operations {
            let key = format!("{}{}", if ty & 1 == 0 { 't' } else { 'e' }, ty / 2);
            ensure!(
                self.world.types.contains_key(&key),
                "Endpoint {name} is outside the immutable manifest: {key}"
            );
            let stem = name.strip_suffix("_t").unwrap();
            writeln!(
                self.source,
                "case {ty}: {stem}_drop_readable(handle); break;"
            )?;
        }
        self.source.push_str("default: __builtin_trap();\n}\nperry_producer_reader_dropped(type, handle);\n}\nperry_component_frame_drop(f);\nif (status < 0) __builtin_trap();\nperry_producer_flush();\nperry_endpoint_flushing = false;\n}\n");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experimental_endpoint_adapters_compile_for_all_generic_value_worlds() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-wasi/components/async-guest-values/world.wit");
        for world in ["plugin", "provider", "supervisor"] {
            assert!(canonical_guest(&fixture, Some(world))
                .unwrap_err()
                .to_string()
                .contains("future/stream guest values"));
            let files = experimental_async_guest(&fixture, Some(world)).unwrap();
            wit_component::metadata::decode(&files[&format!("{world}_component_type.o")]).expect(
                "generated canonical world must decode without type/function name collisions",
            );
            let dir = tempfile::tempdir().unwrap();
            for (name, bytes) in files {
                std::fs::write(dir.path().join(name), bytes).unwrap();
            }
            let output = std::process::Command::new("clang")
                .args([
                    "-std=c11",
                    "-Wall",
                    "-Werror",
                    "-Wno-unused-function",
                    "-fsyntax-only",
                ])
                .arg(dir.path().join("perry_adapter.c"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{world}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn endpoint_values_in_compound_arguments_and_results_keep_the_public_guard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.wit");
        std::fs::write(
            &path,
            r#"package test:compound-endpoints;
            interface api {
                resource item;
                record values { a: future<own<item>>, b: stream<future<string>> }
                type batch = list<result<values, future>>;
                exchange: async func(value: batch) -> batch;
            }
            world plugin { import api; export api; }
        "#,
        )
        .unwrap();
        assert!(canonical_guest(&path, None)
            .unwrap_err()
            .to_string()
            .contains("future/stream"));
        let files = experimental_async_guest(&path, None).unwrap();
        wit_component::metadata::decode(&files["plugin_component_type.o"])
            .expect("generated compound world must decode");
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let output = std::process::Command::new("clang")
            .args([
                "-std=c11",
                "-Wall",
                "-Werror",
                "-Wno-unused-function",
                "-fsyntax-only",
            ])
            .arg(dir.path().join("perry_adapter.c"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn scheduler_signal_type_and_function_names_avoid_public_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.wit");
        std::fs::write(&path, "package test:signal-names; world plugin { type perry-scheduler-signal-0 = u32; export perry-scheduler-signal-1: func(); export work: async func(value: stream) -> stream; }").unwrap();
        let files = experimental_async_guest(&path, None).unwrap();
        let (_, metadata) = wit_component::metadata::decode(&files["plugin_component_type.o"])
            .expect("type and function namespaces must remain valid");
        assert!(metadata.resolve.worlds[metadata.world]
            .exports
            .contains_key(&WorldKey::Name("perry-scheduler-signal-2".into())));
    }
}
