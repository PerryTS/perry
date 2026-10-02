//! Cache cells containing heap handles must have the same agent ownership
//! as literal pools, even in a helper module that imports no thread API.
use perry_codegen::{
    compile_module, set_program_has_thread_agents, set_program_has_worker, CompileOptions,
};
use perry_hir::{BinaryOp, Expr, Module, Stmt};

#[test]
fn concat_site_cells_and_fill_address_are_agent_local_in_worker_graphs() {
    let mut module = Module::new("concat_helper.ts");
    module.init.push(Stmt::Expr(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("worker-".into())),
        right: Box::new(Expr::Integer(1)),
    }));
    // This integration target has one test, so its process-wide compilation
    // graph flags cannot race with unrelated compilation tests.
    for (workers, agents) in [(false, false), (true, false), (false, true)] {
        set_program_has_worker(workers);
        set_program_has_thread_agents(agents);
        let opts = CompileOptions {
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
    set_program_has_worker(false);
    set_program_has_thread_agents(false);
}
