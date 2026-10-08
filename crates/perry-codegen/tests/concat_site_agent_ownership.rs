//! Cache cells containing heap handles must have the same agent ownership
//! as literal pools, even in a helper module that imports no thread API.
use perry_codegen::{compile_module, CompileOptions};
use perry_hir::{BinaryOp, Expr, Module, Stmt};

#[test]
fn concat_site_cells_and_fill_address_are_agent_local_in_worker_graphs() {
    let mut module = Module::new("concat_helper.ts");
    module.init.push(Stmt::Expr(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("worker-".into())),
        right: Box::new(Expr::Integer(1)),
    }));
    for (workers, agents) in [(false, false), (true, false), (false, true)] {
        let opts = CompileOptions {
            program_has_worker: workers,
            program_has_thread_agents: agents,
            emit_ir_only: true,
            is_entry_module: false,
            ..Default::default()
        };
        let ir = String::from_utf8(compile_module(&module, opts).unwrap()).unwrap();
        let table = ir
            .lines()
            .find(|line| line.starts_with("@perry_concat_site_"))
            .expect("fixture must exercise the concat-site cache");
        assert_eq!(table.contains("thread_local global"), workers || agents);
        assert!(table.contains("[32 x i64] zeroinitializer"), "{table}");
        let symbol = table.split(" =").next().unwrap();
        assert!(
            ir.lines()
                .any(|line| line.contains("getelementptr [32 x i64]") && line.contains(symbol)),
            "inline probe must address that same table"
        );
        assert!(
            ir.lines()
                .any(|line| line.contains("ptrtoint ptr") && line.contains(symbol)),
            "miss helper must receive that same table address"
        );
        assert!(ir.contains("call double @js_string_concat_site_value("));
        assert!(!ir
            .lines()
            .any(|line| line.starts_with(symbol) && line.contains(" constant ")));
    }
}

// A helper imports no launch API: only the whole-program compile options
// determine ownership. All four combinations must match their serial IR.
#[test]
fn concurrent_compiles_have_independent_agent_ownership() {
    let compile = |workers, agents| {
        let mut module = Module::new("ownership_helper.ts");
        module.init.push(Stmt::Expr(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("agent-".into())),
            right: Box::new(Expr::Integer(1)),
        }));
        String::from_utf8(
            compile_module(
                &module,
                CompileOptions {
                    target: Some("x86_64-unknown-linux-gnu".into()),
                    program_has_worker: workers,
                    program_has_thread_agents: agents,
                    emit_ir_only: true,
                    is_entry_module: false,
                    ..Default::default()
                },
            )
            .expect("compile"),
        )
        .expect("IR")
    };
    let modes = [(false, false), (true, false), (false, true), (true, true)];
    let references: Vec<_> = modes.iter().map(|&(w, a)| compile(w, a)).collect();
    for (i, &(workers, agents)) in modes.iter().enumerate() {
        let ir = &references[i];
        let table = ir
            .lines()
            .find(|l| l.starts_with("@perry_concat_site_"))
            .unwrap();
        assert_eq!(table.contains("thread_local"), workers || agents);
        let guard = ir
            .lines()
            .find(|l| l.starts_with("@__perry_init_done_"))
            .unwrap();
        assert_eq!(guard.contains("thread_local"), workers);
        let literals = ir
            .lines()
            .find(|l| l.starts_with("@__perry_literals_ready_"))
            .unwrap();
        assert_eq!(literals.contains("thread_local"), workers);
    }
    assert_ne!(references[0], references[1]);
    assert_ne!(references[0], references[2]);
    assert_ne!(references[1], references[2]);
    let barrier = std::sync::Barrier::new(modes.len());
    std::thread::scope(|scope| {
        for (i, &(workers, agents)) in modes.iter().enumerate() {
            let reference = &references[i];
            let barrier = &barrier;
            scope.spawn(move || {
                for round in 0..24 {
                    barrier.wait();
                    assert_eq!(
                        &compile(workers, agents),
                        reference,
                        "workers={workers}, agents={agents}, round={round}"
                    );
                }
            });
        }
    });
}
