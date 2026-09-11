//! Minimal native entry. Only a proved program can produce its output table.
//! output.c consumes this table without allocation or managed runtime calls.
use std::fmt::Write;

use super::analysis::ProvenProgram;

pub(super) fn llvm_ir(program: &ProvenProgram) -> String {
    let mut ir = String::from("; Perry proven constant-string program: no managed runtime\n");
    for (index, output) in program.outputs().iter().enumerate() {
        let bytes: String = output.bytes.iter().map(|b| format!("\\{b:02X}")).collect();
        writeln!(ir, "@perry_tiny_text_{index} = private unnamed_addr constant [{} x i8] c\"{bytes}\", align 1", output.bytes.len()).unwrap();
    }
    if !program.outputs().is_empty() {
        ir.push_str("%perry.tiny.output = type { i32, ptr, i64 }\n");
        writeln!(
            ir,
            "@perry_tiny_outputs = private constant [{} x %perry.tiny.output] [",
            program.outputs().len()
        )
        .unwrap();
        for (index, output) in program.outputs().iter().enumerate() {
            let separator = if index + 1 == program.outputs().len() {
                ""
            } else {
                ","
            };
            writeln!(ir, "  %perry.tiny.output {{ i32 {}, ptr @perry_tiny_text_{index}, i64 {} }}{separator}", output.fd, output.bytes.len()).unwrap();
        }
        ir.push_str("]\ndeclare void @perry_tiny_run_output(ptr, i64)\n");
    }
    ir.push_str("define i32 @main(i32 %argc, ptr %argv) {\nentry:\n");
    if !program.outputs().is_empty() {
        writeln!(
            ir,
            "  call void @perry_tiny_run_output(ptr @perry_tiny_outputs, i64 {})",
            program.outputs().len()
        )
        .unwrap();
    }
    ir.push_str("  ret i32 0\n}\n");
    ir
}

#[cfg(test)]
mod tests {
    use super::super::analysis::analyze;
    use super::*;

    #[test]
    fn empty_entry_has_no_external_dependencies() {
        let ir = llvm_ir(&analyze("", "app.ts").unwrap());
        assert!(!ir.contains("declare "));
        assert!(!ir.contains("call "));
        assert!(ir.contains("ret i32 0"));
    }

    #[test]
    fn output_has_only_the_audited_native_helper_dependency() {
        let ir = llvm_ir(&analyze("console.log('a\\0%'); console.error('b');", "app.ts").unwrap());
        let declarations: Vec<_> = ir
            .lines()
            .filter(|line| line.starts_with("declare "))
            .collect();
        assert_eq!(
            declarations,
            ["declare void @perry_tiny_run_output(ptr, i64)"]
        );
        assert!(ir.contains(r#"c"\61\00\25\0A""#));
        for excluded in [
            "@js_",
            "@perry_gc",
            "mimalloc",
            "@malloc",
            "@calloc",
            "promise",
            "shadow",
            "statepoint",
        ] {
            assert!(!ir.contains(excluded), "unexpected dependency {excluded}");
        }
    }
}
