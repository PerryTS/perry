//! Literal chunks (`split_regions_literal.rs`): which giant literal births
//! move their values into chunk functions, and what those functions hold.

use perry_hir::{Expr, Function, Module, Stmt};

use super::split_regions::{is_region_function, split_large_bodies_with, Limits};

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            perry_hir::lower_module(&parsed.module, "test", "test.ts")
                .expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// Small limits: a literal whose values past the head are over 60 nodes is
/// giant, and a chunk holds about 30 nodes.
fn tiny() -> Limits {
    Limits {
        min_body: 10,
        target: 30,
        guard_min: 4,
        chunk_min: 4,
        closures: false,
        sink: true,
        typed: true,
        max_regions: usize::MAX,
        interface_nodes: 0,
    }
}

/// `{ <name>0: <value(0)>, … }` with `n` properties.
fn literal(n: usize, name: &str, value: impl Fn(usize) -> String) -> String {
    let props: Vec<String> = (0..n).map(|i| format!("{name}{i}: {}", value(i))).collect();
    format!("{{ {} }}", props.join(", "))
}

/// Every `New` argument list in the module's top-level statements and
/// function bodies (the positions a chunked birth sits in).
fn new_args(m: &Module) -> Vec<&Vec<Expr>> {
    fn collect<'a>(e: &'a Expr, out: &mut Vec<&'a Vec<Expr>>) {
        if let Expr::New { args, .. } = e {
            out.push(args);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| collect(c, out));
    }
    let mut out = Vec::new();
    let bodies = std::iter::once(&m.init).chain(m.functions.iter().map(|f| &f.body));
    for body in bodies {
        for s in body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                collect(e, &mut out);
                false
            });
        }
    }
    out
}

/// A number literal of value `v` (integers lower to `Integer`).
fn is_num(e: &Expr, v: usize) -> bool {
    match e {
        Expr::Number(n) => *n == v as f64,
        Expr::Integer(n) => *n == v as i64,
        _ => false,
    }
}

fn has_chunks(m: &Module) -> bool {
    new_args(m)
        .iter()
        .any(|args| args.iter().any(|a| matches!(a, Expr::LiteralChunks(_))))
}

/// The chunked birth's arguments: the one `New` with chunk markers.
fn chunked_args(m: &Module) -> &Vec<Expr> {
    let found: Vec<_> = new_args(m)
        .into_iter()
        .filter(|args| args.iter().any(|a| matches!(a, Expr::LiteralChunks(_))))
        .collect();
    assert_eq!(found.len(), 1, "one chunked birth");
    found[0]
}

/// `(key, slot, value)` of every slot init, chunk by chunk in call order.
fn slot_inits<'a>(m: &'a Module, calls: &[Expr]) -> Vec<(&'a str, u32, &'a Expr, &'a Function)> {
    let mut out = Vec::new();
    for call in calls {
        let Expr::Call { callee, args, .. } = call else {
            panic!("a chunk is a call: {call:?}");
        };
        let Expr::FuncRef(fid) = callee.as_ref() else {
            panic!("a chunk is a direct call");
        };
        assert!(
            matches!(args.first(), Some(Expr::LiteralChunkObject)),
            "the object is argument 0"
        );
        let f = m
            .functions
            .iter()
            .find(|f| f.id == *fid)
            .expect("chunk function");
        assert!(is_region_function(f), "{} is a region", f.name);
        assert_eq!(f.params.len(), args.len());
        for s in &f.body {
            let Stmt::Expr(Expr::LiteralSlotInit {
                object,
                key,
                slot,
                value,
            }) = s
            else {
                panic!("a chunk body is slot inits: {s:?}");
            };
            assert!(
                matches!(object.as_ref(), Expr::LocalGet(id) if *id == f.params[0].id),
                "a slot init stores into the object parameter"
            );
            out.push((key.as_str(), *slot, value.as_ref(), f));
        }
    }
    out
}

#[test]
fn a_giant_module_literal_moves_its_tail_into_chunks_in_key_order() {
    let src = format!(
        "function mk(i: number, s: string) {{ return {{ i, s }}; }}\n\
         export const D = {};\n\
         console.log(D.k0, D.k99);\n",
        literal(100, "k", |i| format!("mk({i}, \"s{i}\")"))
    );
    let mut m = lower_src(&src);
    split_large_bodies_with(&mut m, tiny());
    let args = chunked_args(&m);
    assert_eq!(args.len(), 100, "the birth keeps every slot");
    // The head is untouched: the birth still evaluates slots 0..32 itself.
    for (i, a) in args[..32].iter().enumerate() {
        let Expr::Call {
            args: call_args, ..
        } = a
        else {
            panic!("head slot {i} keeps its value: {a:?}");
        };
        assert!(is_num(&call_args[0], i), "head slot {i}");
    }
    let Expr::LiteralChunks(calls) = &args[32] else {
        panic!("slot 32 holds the chunk calls");
    };
    assert!(calls.len() >= 2, "more than one chunk: {}", calls.len());
    assert!(args[33..]
        .iter()
        .all(|a| matches!(a, Expr::LiteralChunks(c) if c.is_empty())));
    // Every chunked slot is initialized exactly once, in source order, with
    // its own key and its own value.
    let inits = slot_inits(&m, calls);
    assert_eq!(inits.len(), 68);
    for (k, (key, slot, value, _)) in inits.iter().enumerate() {
        let i = 32 + k;
        assert_eq!(*slot as usize, i);
        assert_eq!(*key, format!("k{i}"));
        let Expr::Call {
            args: call_args, ..
        } = value
        else {
            panic!("slot {i}'s value is its call: {value:?}");
        };
        assert!(is_num(&call_args[0], i), "slot {i}");
        assert!(matches!(&call_args[1], Expr::String(s) if *s == format!("s{i}")));
    }
}

#[test]
fn a_function_literal_passes_its_live_ins_by_value() {
    let src = format!(
        "function build(x: number, s: string) {{ const o = {}; return o; }}\n\
         console.log(build(1, 'a').f50);\n",
        literal(100, "f", |i| format!("[x, s, {i}]"))
    );
    let mut m = lower_src(&src);
    split_large_bodies_with(&mut m, tiny());
    let Expr::LiteralChunks(calls) = &chunked_args(&m)[32] else {
        panic!("slot 32 holds the chunk calls");
    };
    let build = m
        .functions
        .iter()
        .find(|f| f.name == "build")
        .expect("build");
    let x = build.params[0].id;
    let s = build.params[1].id;
    for call in calls {
        let Expr::Call { args, .. } = call else {
            unreachable!()
        };
        let live: Vec<u32> = args[1..]
            .iter()
            .map(|a| match a {
                Expr::LocalGet(id) => *id,
                other => panic!("a live-in is a local read: {other:?}"),
            })
            .collect();
        assert!(live.contains(&x) && live.contains(&s), "{live:?}");
    }
    assert_eq!(slot_inits(&m, calls).len(), 68);
}

/// A literal that keeps today's lowering: no chunk markers anywhere, and its
/// birth (if it is one) still carries every value.
fn assert_not_chunked(src: &str) {
    let mut m = lower_src(src);
    split_large_bodies_with(&mut m, tiny());
    assert!(!has_chunks(&m), "not chunked:\n{src}");
    assert!(
        !m.functions.iter().any(|f| f
            .body
            .iter()
            .any(|s| matches!(s, Stmt::Expr(Expr::LiteralSlotInit { .. })))),
        "no chunk function was left behind"
    );
}

fn calls(n: usize) -> String {
    literal(n, "k", |i| format!("mk({i})"))
}

const MK: &str = "function mk(i: number) { return { i }; }\n";

#[test]
fn a_getter_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}export const D = {{ get g() {{ return 1; }}, {} }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn a_setter_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}let v = 0;\nexport const D = {{ set g(x: number) {{ v = x; }}, {} }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn a_computed_key_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}const key = 'c' + 1;\nexport const D = {{ {}, [key]: 1 }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn a_spread_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}const o = {{ a: 1 }};\nexport const D = {{ ...o, {} }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn a_duplicate_key_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}export const D = {{ {}, k40: mk(1000) }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn a_colon_proto_key_is_not_chunked() {
    let body = calls(100);
    assert_not_chunked(&format!(
        "{MK}const p = {{ q: 1 }};\nexport const D = {{ {}, __proto__: p }};\n",
        &body[2..body.len() - 2]
    ));
}

#[test]
fn function_values_move_into_their_chunks() {
    let src = format!(
        "{MK}function top() {{ return 3; }}\nconst D = {};\nconsole.log(D);\n",
        literal(100, "k", |i| match i {
            60 => "(v: number) => v + 1".to_string(),
            61 => "function (this: any) { return this; }".to_string(),
            62 => "top".to_string(),
            _ => format!("mk({i})"),
        })
    );
    let mut m = lower_src(&src);
    split_large_bodies_with(&mut m, tiny());
    let Expr::LiteralChunks(calls) = &chunked_args(&m)[32] else {
        panic!("slot 32 holds the chunk calls");
    };
    let inits = slot_inits(&m, calls);
    assert_eq!(inits.len(), 68);
    assert!(matches!(
        inits[60 - 32].2,
        Expr::Closure { is_arrow: true, .. }
    ));
    assert!(matches!(
        inits[61 - 32].2,
        Expr::Closure {
            is_arrow: false,
            ..
        }
    ));
    assert!(!matches!(inits[62 - 32].2, Expr::Call { .. }));
}

#[test]
fn function_values_in_the_head_stay_in_the_birth() {
    let src = format!(
        "{MK}const D = {};\nconsole.log(D);\n",
        literal(100, "k", |i| if i < 2 {
            format!("(v: number) => v + {i}")
        } else {
            format!("mk({i})")
        })
    );
    let mut m = lower_src(&src);
    split_large_bodies_with(&mut m, tiny());
    let args = chunked_args(&m);
    assert!(matches!(args[0], Expr::Closure { .. }) && matches!(args[1], Expr::Closure { .. }));
    let Expr::LiteralChunks(calls) = &args[32] else {
        panic!("slot 32 holds the chunk calls");
    };
    assert_eq!(slot_inits(&m, calls).len(), 68);
}

#[test]
fn a_closure_capturing_this_is_not_chunked() {
    assert_not_chunked(&format!(
        "{MK}function f(this: any) {{ return {}; }}\n",
        literal(100, "k", |i| if i == 60 {
            "() => this".to_string()
        } else {
            format!("mk({i})")
        })
    ));
}

#[test]
fn a_raw_number_lane_is_not_chunked() {
    assert_not_chunked(&format!(
        "function nums(x: number) {{ return {}; }}\nconsole.log(nums(1).k5);\n",
        literal(100, "k", |i| format!("x + {i}"))
    ));
}

#[test]
fn a_this_read_is_not_chunked() {
    assert_not_chunked(&format!(
        "{MK}class C {{ n = 1; m() {{ return {}; }} }}\nconsole.log(new C().m().k3);\n",
        literal(100, "k", |i| if i == 70 {
            "this.n".to_string()
        } else {
            format!("mk({i})")
        })
    ));
}

#[test]
fn an_await_in_a_value_is_not_chunked() {
    assert_not_chunked(&format!(
        "{MK}async function a(p: Promise<number>) {{ return {}; }}\na(Promise.resolve(1));\n",
        literal(100, "k", |i| if i == 50 {
            "await p".to_string()
        } else {
            format!("mk({i})")
        })
    ));
}

#[test]
fn a_value_writing_a_later_read_local_is_not_chunked() {
    // `n` is written by slot 40's value and read after the literal: a chunk
    // would have to hand it back, which a slot-init chunk never does.
    assert_not_chunked(&format!(
        "{MK}function f() {{ let n = 0; const o = {}; return [o, n]; }}\nconsole.log(f());\n",
        literal(100, "k", |i| if i == 40 {
            "(n = 5)".to_string()
        } else {
            format!("mk({i})")
        })
    ));
}

#[test]
fn a_small_literal_is_not_chunked() {
    assert_not_chunked(&format!("{MK}export const D = {};\n", calls(40)));
}

#[test]
fn a_refused_literal_leaves_no_trace() {
    // Too small, a duplicate key, and a refused chunk (`this`): the
    // module after splitting is the module before it, ids included.
    for src in [
        format!("{MK}export const D = {};\n", calls(40)),
        {
            let body = calls(100);
            format!(
                "{MK}export const D = {{ {}, k40: mk(1000) }};\n",
                &body[2..body.len() - 2]
            )
        },
        format!(
            "{MK}function f(this: any) {{ return {}; }}\n",
            literal(100, "k", |i| if i == 70 {
                "this.n".to_string()
            } else {
                format!("mk({i})")
            })
        ),
    ] {
        let mut m = lower_src(&src);
        let snapshot = |m: &Module| {
            let spans: std::collections::BTreeMap<_, _> = m.local_source_spans.iter().collect();
            format!("{:?}", (&m.init, &m.functions, spans))
        };
        let before = snapshot(&m);
        split_large_bodies_with(&mut m, tiny());
        let after = snapshot(&m);
        assert!(before == after, "unchanged:\n{src}");
    }
}
