//! COFF cannot coalesce a bare ODR weak function across object files: LLVM
//! emits a weak external alias whose default includes a module-local symbol.
//! Give shared helpers a named COMDAT so MSVC can select one actual body.
use inkwell::comdat::ComdatSelectionKind;
use inkwell::module::{Linkage, Module};

pub(super) fn coalesce_helpers(module: &Module<'_>, target: &str) {
    if crate::codegen::target_os_of(target) != Some("windows") {
        return;
    }
    for function in module.get_functions() {
        let global = function.as_global_value();
        if function.count_basic_blocks() == 0
            || !matches!(
                function.get_linkage(),
                Linkage::LinkOnceODR | Linkage::WeakODR
            )
            || global.get_comdat().is_some()
        {
            continue;
        }
        let group = module.get_or_insert_comdat(&function.get_name().to_string_lossy());
        group.set_selection_kind(ComdatSelectionKind::Any);
        global.set_comdat(group);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IR: &str = "
        define weak_odr i64 @perry_birth_class(i64 %x) noinline {
            ret i64 %x
        }
        define i64 @entry(i64 %x) {
            %r = call i64 @perry_birth_class(i64 %x)
            ret i64 %r
        }
        declare i64 @external_helper(i64)
    ";

    #[test]
    fn coff_shared_helper_emits_a_comdat_body_not_a_weak_alias() {
        let objects = crate::inprocess::compile_ll_to_object_inprocess(
            IR,
            "x86_64-pc-windows-msvc",
            &["-O0".into(), "-S".into()],
            "coff-helper",
            false,
        )
        .expect("COFF helper assembles");
        let assembly = String::from_utf8(objects.concat()).unwrap();
        assert!(assembly.lines().any(|line| line.contains(".section")
            && line.contains("discard,perry_birth_class")), "{assembly}");
        assert!(!assembly.contains(".weak.perry_birth_class"), "{assembly}");
    }

    #[test]
    fn only_windows_odr_definitions_gain_comdats() {
        let context = inkwell::context::Context::create();
        let module = crate::inprocess::parse_ir_text(&context, IR, "helpers").unwrap();
        for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            coalesce_helpers(&module, target);
            assert!(module
                .get_function("perry_birth_class")
                .unwrap()
                .as_global_value()
                .get_comdat()
                .is_none());
        }
        coalesce_helpers(&module, "x86_64-pc-windows-msvc");
        let helper = module
            .get_function("perry_birth_class")
            .unwrap()
            .as_global_value();
        let group = helper.get_comdat().unwrap();
        coalesce_helpers(&module, "x86_64-pc-windows-msvc");
        assert_eq!(helper.get_comdat(), Some(group));
        for name in ["entry", "external_helper"] {
            assert!(module
                .get_function(name)
                .unwrap()
                .as_global_value()
                .get_comdat()
                .is_none());
        }
        module.verify().unwrap();
    }
}
