//! The reflect-metadata install leads module init without moving class windows.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &'static str) -> Module {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(src, "reflect.ts", &mut cache)
                .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "reflect.ts").expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn decl_stmt(module: &Module) -> u32 {
    module
        .classes
        .iter()
        .find(|c| c.name == "A")
        .and_then(|c| c.decl_stmt)
        .expect("A")
}

#[test]
fn install_before_a_lowered_class_shifts_its_decl_stmt() {
    let plain = lower("class A {}\n");
    let imported = lower("class A {}\nimport \"reflect-metadata\";\n");
    assert!(!crate::installs_reflect_metadata(&plain));
    assert!(crate::installs_reflect_metadata(&imported));
    assert_eq!(decl_stmt(&imported), decl_stmt(&plain) + 1);
}

#[test]
fn program_install_is_added_once() {
    let mut module = lower("class A {}\n");
    let before = decl_stmt(&module);
    crate::prepend_reflect_metadata_install(&mut module);
    let len = module.init.len();
    crate::prepend_reflect_metadata_install(&mut module);
    assert!(crate::installs_reflect_metadata(&module));
    assert_eq!(module.init.len(), len);
    assert_eq!(decl_stmt(&module), before + 1);
}
