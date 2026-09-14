use crate::{Expr, Stmt};

fn lower(source: &str) -> crate::Module {
    let mut cache = perry_diagnostics::SourceCache::new();
    let parsed =
        perry_parser::parse_typescript_with_cache(source, "hoist.cts", &mut cache).unwrap();
    crate::lower_module(&parsed.module, "hoist", "hoist.cts").unwrap()
}

#[test]
fn sloppy_block_initialization_precedes_reads_but_outer_copy_stays_at_declaration() {
    let module = lower(
        r#"
        function test() {
            for (let i = 0; i < 2; i++) {
                const before = read;
                function read() { return i; }
                const after = read;
            }
            return read;
        }
    "#,
    );
    let function = module.functions.iter().find(|f| f.name == "test").unwrap();
    let body = function
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::For { body, .. } => Some(body),
            _ => None,
        })
        .unwrap();
    let (init_position, inner_id) = body
        .iter()
        .enumerate()
        .find_map(|(pos, stmt)| match stmt {
            Stmt::Let {
                id,
                name,
                init: Some(Expr::Closure { .. }),
                ..
            } if name == "read" => Some((pos, *id)),
            _ => None,
        })
        .unwrap();
    let read_position = body.iter().position(|s| matches!(s,
        Stmt::Let { name, init: Some(Expr::LocalGet(id)), .. } if name == "before" && *id == inner_id
    )).expect("a pre-declaration read must resolve to the block-local function");
    let (copy_position, outer_id) = body.iter().enumerate().find_map(|(pos, stmt)| match stmt {
        Stmt::Expr(Expr::LocalSet(outer, value)) if matches!(value.as_ref(), Expr::LocalGet(id) if *id == inner_id) => Some((pos, *outer)),
        _ => None,
    }).unwrap();
    assert!(
        init_position < read_position && read_position < copy_position,
        "{body:#?}"
    );
    assert_ne!(inner_id, outer_id, "Annex B must keep two bindings");
    assert!(function.body.iter().any(|s| matches!(s,
        Stmt::Return(Some(Expr::LocalGet(id))) if *id == outer_id
    )));
}

#[test]
fn block_function_shadows_a_parameter_in_both_strictness_modes() {
    for directive in ["", "'use strict';"] {
        let source = format!(
            r#"
            function test(read) {{
                {directive}
                {{
                    const before = read;
                    function read() {{ return 7; }}
                }}
                return read;
            }}
        "#
        );
        let module = lower(&source);
        let function = module.functions.iter().find(|f| f.name == "test").unwrap();
        let parameter = function.params[0].id;
        let inner_id = function
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::Let {
                    id,
                    name,
                    init: Some(Expr::Closure { .. }),
                    ..
                } if name == "read" => Some(*id),
                _ => None,
            })
            .unwrap();
        assert_ne!(inner_id, parameter, "directive={directive}");
        assert!(function.body.iter().any(|s| matches!(s,
            Stmt::Let { name, init: Some(Expr::LocalGet(id)), .. } if name == "before" && *id == inner_id
        )), "the early read must use the shadowing block binding");
        assert!(function.body.iter().any(|s| matches!(s,
            Stmt::Return(Some(Expr::LocalGet(id))) if *id == parameter
        )));
    }
}
