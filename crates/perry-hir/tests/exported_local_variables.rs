use perry_hir::lower_module;
use perry_parser::parse_typescript;

fn exported_variables(source: &str) -> Vec<String> {
    let parsed = parse_typescript(source, "exports.ts").expect("parse");
    lower_module(&parsed, "exports", "exports.ts")
        .expect("lower")
        .exported_objects
}

#[test]
fn local_exports_do_not_depend_on_initializer_shape() {
    for init in [
        "new Set()",
        "new Set(['doctor'])",
        "new Map()",
        "new Map([['doctor', true]])",
        "1 + 2",
        "true ? 1 : 2",
        "!false",
        "new Date(0)",
        "42",
        "undefined",
        "() => 42",
    ] {
        for declaration in ["var", "let", "const"] {
            for forward in [false, true] {
                let decl = format!("{declaration} commands = {init};");
                let export = "export { commands as COMMANDS };";
                let source = if forward {
                    format!("{export} {decl}")
                } else {
                    format!("{decl} {export}")
                };
                let names = exported_variables(&source);
                for name in ["commands", "COMMANDS"] {
                    assert!(
                        names.iter().any(|n| n == name),
                        "missing {name}: {source}: {names:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn uninitialized_and_destructured_local_exports_get_storage() {
    for source in [
        "let value; export { value as publicValue };",
        "export { value as publicValue }; var value;",
        "const [value] = [42]; export { value as publicValue };",
        "export { value as publicValue }; const { value } = { value: 42 };",
        "const value = new Set(); export { value };",
    ] {
        let names = exported_variables(source);
        assert!(names.iter().any(|n| n == "value"), "{source}: {names:?}");
    }
}

#[test]
fn imports_functions_and_types_are_not_local_variable_exports() {
    let names = exported_variables(
        r#"
        import { remote } from './remote';
        function declared() { return 42; }
        type Shape = { value: number };
        export { remote as REMOTE, declared as DECLARED };
        export type { Shape };
        function hidden() { const local = 42; return local; }
    "#,
    );
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn block_bindings_do_not_reclassify_import_or_function_exports() {
    for shadow in [
        "try { const remote = 1; } finally {}",
        "try {} finally { const remote = 1; }",
        "try { throw 1; } catch (error) { const remote = 1; }",
        "try { const [remote] = [1]; } finally {}",
    ] {
        for declaration in [
            "import { remote } from './remote';",
            "function remote() { return 42; }",
        ] {
            let source = format!("{declaration} {shadow} export {{ remote as REMOTE }};");
            let names = exported_variables(&source);
            assert!(names.is_empty(), "{source}: {names:?}");
        }
    }
}
