//! A label no jump targets must not change how a module-level loop lowers:
//! the labelled loop is the unlabelled loop's HIR wrapped in `Labeled`. A
//! labelled module-level loop used to lower as a function-body statement
//! (block bodies spliced, receiver hoists), so the label alone changed the
//! loop's HIR and with it every tier decision downstream.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module, Stmt};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "labelled_loop.ts", &mut cache)
                    .expect("parse should succeed");
            lower_module(&parsed.module, "test", "labelled_loop.ts").expect("lower")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// `{P}` is four spaces (plain) or `lbl:` (labelled): same length, so every
/// byte offset in the two lowerings is equal.
const LOOPS: [&str; 3] = [
    "const buf = new Uint8Array(8);\nlet h = 0;\n{P}for (let i = 0; i < buf.length; i++) { const a = buf[i]; h = (h ^ a) | 0; }\nconsole.log(h);\n",
    "let n = 0;\n{P}while (n < 4) { const a = n; n = n + 1 + a * 0; }\nconsole.log(n);\n",
    "let m = 0;\n{P}do { const a = m; m = m + 1 + a * 0; } while (m < 4);\nconsole.log(m);\n",
];

#[test]
fn a_labelled_module_loop_lowers_as_the_unlabelled_loop() {
    for src in LOOPS {
        let plain = lower(&src.replace("{P}", "    "));
        let labelled = lower(&src.replace("{P}", "lbl:"));
        let mut unwrapped = labelled.init.clone();
        let mut labels = 0;
        for stmt in unwrapped.iter_mut() {
            if let Stmt::Labeled { label, body } = stmt {
                assert_eq!(label, "lbl");
                let body = std::mem::replace(body.as_mut(), Stmt::Break);
                *stmt = body;
                labels += 1;
            }
        }
        assert_eq!(labels, 1, "the label is on the loop: {src}");
        assert_eq!(
            format!("{:?}", plain.init),
            format!("{:?}", unwrapped),
            "label changed the loop HIR: {src}"
        );
    }
}

/// A module-level loop owns its block's statement list as a loop in a
/// function body does: the body is the block's statements, not a lexical
/// `if (true)` wrapper around them.
#[test]
fn a_module_loop_body_is_the_block_list() {
    for src in LOOPS {
        let module = lower(&src.replace("{P}", "    "));
        let body = module
            .init
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    Some(body)
                }
                _ => None,
            })
            .expect("loop");
        assert!(
            matches!(body.first(), Some(Stmt::Let { .. })),
            "module loop body wrapped: {src}: {:?}",
            body.first()
        );
    }
}
