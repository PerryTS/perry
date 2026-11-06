//! `this` receivers in class-field loops: `this.f op= v` is written in place,
//! and a declared array field read in a call-free loop is hoisted behind a
//! `this instanceof C` version check. Behaviour is covered against node by
//! `tests/parity/language/class/test_class_this_*.ts`; these pin the shapes.

#![cfg(test)]

use crate::ir::{Expr, Stmt};
use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "t.ts", &mut cache)
                .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "t.ts").expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn method<'m>(m: &'m Module, class: &str, name: &str) -> &'m [Stmt] {
    let c = m.classes.iter().find(|c| c.name == class).expect("class");
    &c.methods
        .iter()
        .find(|f| f.name == name)
        .expect("method")
        .body
}

fn debug(stmts: &[Stmt]) -> String {
    format!("{stmts:?}")
}

#[test]
fn a_this_compound_update_needs_no_base_temp() {
    let m = lower(
        "class P { x = 1; bump(n: number) { for (let i = 0; i < n; i++) { this.x += i; this.x ||= 3; } } }\n",
    );
    let body = debug(method(&m, "P", "bump"));
    assert!(!body.contains("__cmpd_base"), "{body}");
    assert!(
        body.contains("PropertySet { object: This, property: \"x\""),
        "the write must target `this` itself: {body}"
    );
}

#[test]
fn a_computed_key_on_this_keeps_its_temps() {
    // The key's evaluation must stay ordered after the base read.
    let m = lower("class P { x = 1; k(s: string) { this[s as 'x'] += 1; } }\n");
    let body = debug(method(&m, "P", "k"));
    assert!(body.contains("__cmpd_"), "{body}");
}

fn version_check(stmts: &[Stmt]) -> Option<(&[Stmt], &[Stmt])> {
    stmts.iter().find_map(|s| match s {
        Stmt::If {
            condition: Expr::ThisProvenClass(class),
            then_branch,
            else_branch: Some(else_branch),
        } if class == "B" => Some((then_branch.as_slice(), else_branch.as_slice())),
        _ => None,
    })
}

#[test]
fn a_declared_array_field_read_in_a_call_free_loop_is_hoisted() {
    let m = lower(
        "class B { board: number[] = []; h = 0;\n\
         sum(n: number) { let s = 0; for (let i = 0; i < n; i++) { s += this.board[i & 7] as number; this.h ^= i; } return s; } }\n",
    );
    let (fast, original) = version_check(method(&m, "B", "sum")).expect("versioned loop");
    let fast = debug(fast);
    assert!(fast.contains("__perry_hoist_this_board"), "{fast}");
    assert!(
        !fast.contains("property: \"board\", byte_offset: 0, byte_end: 0 }, index"),
        "{fast}"
    );
    assert!(
        debug(original).contains("property: \"board\""),
        "the else arm keeps the original loop"
    );
}

#[test]
fn loops_that_can_change_or_observe_the_field_are_not_hoisted() {
    for body in [
        // A call could replace the array.
        "for (let i = 0; i < n; i++) { s += this.board[i] as number; this.f(); }",
        // A direct write of the field.
        "for (let i = 0; i < n; i++) { s += this.board[i] as number; this.board = []; }",
        // An element write is an index store, outside the whitelist.
        "for (let i = 0; i < n; i++) { this.board[i] = s; }",
        // The initializer runs after the hoist would.
        "for (let i = this.g(); i < n; i++) { s += this.board[i] as number; }",
    ] {
        let src = format!(
            "class B {{ board: number[] = []; f() {{}} g() {{ return 0; }}\n sum(n: number) {{ let s = 0; {body} return s; }} }}\n"
        );
        let m = lower(&src);
        assert!(version_check(method(&m, "B", "sum")).is_none(), "{body}");
    }
}

#[test]
fn an_accessor_or_undeclared_field_is_not_hoisted() {
    for class in [
        "class B { get board(): number[] { return [1]; } sum(n: number) { let s = 0; for (let i = 0; i < n; i++) s += this.board[0] as number; return s; } }",
        "class B { constructor() { (this as any).board = [1]; } sum(n: number) { let s = 0; for (let i = 0; i < n; i++) s += (this as any).board[0]; return s; } }",
    ] {
        let m = lower(class);
        assert!(version_check(method(&m, "B", "sum")).is_none(), "{class}");
    }
}

#[test]
fn reads_that_could_run_user_code_block_the_hoist() {
    for body in [
        // A getter on another receiver could replace the field.
        "for (let i = 0; i < n; i++) { s += this.board[i] as number; s += st.next; }",
        // An element of something other than the hoisted field.
        "for (let i = 0; i < n; i++) { s += (this.board[i] as number) + (other as any)[i]; }",
    ] {
        let src = format!(
            "class B {{ board: number[] = []; sum(n: number, st: any, other: any) {{ let s = 0; {body} return s; }} }}\n"
        );
        let m = lower(&src);
        assert!(version_check(method(&m, "B", "sum")).is_none(), "{body}");
    }
}

#[test]
fn inlining_keeps_every_argument_read_that_could_throw_or_change() {
    for (method_src, call) in [
        // An unused parameter: the argument's TDZ read must not be dropped.
        ("at(sq: number) { return this.board[0] ?? 0; }", "this.at(k)"),
        // A parameter read twice: one read of the argument per call.
        ("at(sq: number) { return (this.board[sq] ?? 0) + sq; }", "this.at(k)"),
    ] {
        let src = format!(
            "class B {{ board: number[] = []; {method_src}\n sum(n: number, k: number) {{ let s = 0; for (let i = 0; i < n; i++) s += {call}; return s; }} }}\n"
        );
        let m = lower(&src);
        assert!(version_check(method(&m, "B", "sum")).is_none(), "{method_src}");
    }
    // A literal argument may be dropped or duplicated.
    let m = lower(
        "class B { board: number[] = []; at(sq: number) { return (this.board[sq] ?? 0) + sq; }\n sum(n: number) { let s = 0; for (let i = 0; i < n; i++) s += this.at(3); return s; } }\n",
    );
    assert!(version_check(method(&m, "B", "sum")).is_some());
}
