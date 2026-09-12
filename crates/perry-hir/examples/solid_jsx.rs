//! Emit the universal expansion for differential testing with a real renderer.
//! cargo run -p perry-hir --example solid_jsx -- input.tsx runtime output.ts

use swc_ecma_visit::VisitMutWith;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: solid_jsx input.tsx runtime output.ts"
    );
    let source = std::fs::read_to_string(&args[1])?;
    let module = perry_parser::parse_typescript(&source, &args[1])?;
    let mut module = perry_hir::solid_jsx::lower_solid_jsx(&module, &args[2]).unwrap_or(module);
    // Exercise the normal HIR path as well as emitting runnable oracle input.
    let hir = perry_hir::lower_module(&module, "solid_oracle", &args[1])?;
    let hir = format!("{hir:?}");
    for name in ["jsx", "jsxs"] {
        anyhow::ensure!(
            !hir.contains(&format!("ExternFuncRef {{ name: {name:?},")),
            "universal JSX left a fallback {name} call in HIR"
        );
    }
    // SWC's printer expects the usual post-transform precedence/paren fixup.
    module.visit_mut_with(&mut swc_ecma_transforms_base::fixer::fixer(None));
    let output = swc_ecma_codegen::to_code(&module);
    std::fs::write(&args[3], output)?;
    Ok(())
}
