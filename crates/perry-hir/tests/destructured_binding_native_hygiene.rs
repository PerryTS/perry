use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<perry_hir::Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(
                &src,
                "destructured_binding_native_hygiene.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            lower_module(
                &parsed.module,
                "test",
                "destructured_binding_native_hygiene.ts",
            )
            .map_err(|error| error.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn destructured_bindings_shadow_stale_native_instance_names() {
    let module = lower_result(
        r#"
        import * as childProcess from "node:child_process";

        let z;
        z = childProcess.spawnSync(process.execPath, ["--version"]);
        function keyed(command) {
            const { install: z } = command;
            z.call(() => {}, {}, []);
        }

        let q;
        q = childProcess.spawnSync(process.execPath, ["--version"]);
        function shorthand(command) {
            const { q } = command;
            q.call(() => {}, {}, []);
        }
        "#,
    )
    .expect("destructured native-name collisions should lower");

    let debug = format!("{module:#?}");
    assert!(
        debug.matches("property: \"call\"").count() >= 2,
        "destructured receivers should keep ordinary property-call lowering: {debug}"
    );
    assert!(
        !debug.contains("method: \"call\""),
        "destructured receivers must not inherit child_process native dispatch: {debug}"
    );
}
