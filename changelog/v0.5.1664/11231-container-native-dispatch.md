### Fixed

- `perry/container`, `perry/compose` and `perry/workloads` calls now reach perry-stdlib's container FFI (#11211). Before, codegen had no dispatch rows for these modules, so every call compiled to `undefined` and the container backend was never invoked. There are now 36 rows in `native_table/container.rs`, plus JS-ABI adapters (`container/js_api.rs`) for signatures with boolean or number flags or with option objects. `perry/container-compose` shares the compose rows.
- `run()` / `create()` now resolve with the documented `{ id, name? }` `ContainerHandle`. Before, they resolved with an id into a registry nothing read. `inspectGraph` accepts the JSON string from `graph()`, as `types/perry/workloads` documents.
- The container docs and the doc snippet now show the real return shapes (JSON strings for list/inspect/logs/exec/listImages/ps).
- A new stub-`docker` end-to-end test, `crates/perry/tests/container_module_dispatch.rs`, asserts the JS results and the exact CLI argv of each call.
