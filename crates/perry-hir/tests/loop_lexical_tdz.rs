use perry_hir::{lower_module, types::LocalId, Stmt};
use perry_parser::parse_typescript;

fn body(source: &str) -> Vec<Stmt> {
    let parsed = parse_typescript(source, "loop-tdz.ts").expect("parse");
    lower_module(&parsed, "test", "loop-tdz.ts")
        .expect("lower")
        .functions
        .into_iter()
        .find(|f| f.name == "test")
        .expect("test function")
        .body
}

fn local(stmts: &[Stmt], name: &str) -> LocalId {
    stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Let { id, name: n, .. } if n == name => Some(*id),
            _ => None,
        })
        .expect("local declaration")
}

#[test]
fn loop_lexicals_allocate_at_block_entry_but_vars_at_function_entry() {
    for directive in ["", "'use strict';"] {
        let stmts = body(&format!(
            r#"
            function test() {{
                {directive}
                for (let i = 0; i < 2; i++) {{
                    const read = () => [value, shared];
                    let value;
                    var shared = i;
                    read();
                }}
            }}
        "#
        ));
        let loop_body = stmts
            .iter()
            .find_map(|s| match s {
                Stmt::For { body, .. } => Some(body),
                _ => None,
            })
            .expect("loop");
        let value = local(loop_body, "value");
        let shared = local(&stmts, "shared");
        assert!(
            matches!(loop_body.first(), Some(Stmt::PreallocateTdzBoxes(ids)) if ids.contains(&value)),
            "lexical cell must be created on each loop entry: {stmts:?}"
        );
        assert!(
            !stmts
                .iter()
                .any(|s| matches!(s, Stmt::PreallocateTdzBoxes(ids) if ids.contains(&value))),
            "nested lexical cell must not be allocated at function entry"
        );
        assert!(
            stmts
                .iter()
                .any(|s| matches!(s, Stmt::PreallocateBoxes(ids) if ids.contains(&shared))),
            "var keeps one function-scoped cell"
        );
    }
}

#[test]
fn strict_hoisted_closure_captures_the_block_entry_tdz_cell() {
    let stmts = body(
        r#"
        function test() {
            'use strict';
            while (true) {
                read();
                function read() { return value; }
                let value = 1;
                break;
            }
        }
    "#,
    );
    let loop_body = stmts
        .iter()
        .find_map(|s| match s {
            Stmt::While { body, .. } => Some(body),
            _ => None,
        })
        .expect("loop");
    let value = local(loop_body, "value");
    assert!(
        matches!(loop_body.first(), Some(Stmt::PreallocateTdzBoxes(ids)) if ids.contains(&value)),
        "TDZ cell must precede hoisted closures: {stmts:?}"
    );
    assert!(
        !loop_body
            .iter()
            .any(|s| matches!(s, Stmt::PreallocateBoxes(ids) if ids.contains(&value))),
        "ordinary closure preallocation must not duplicate the TDZ cell"
    );
}
