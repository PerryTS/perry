//! #11446: mapping callbacks can change an array's element type.
#![cfg(test)]

use crate::types::Type;
use crate::{infer_expr_type, HirTypeEnv};
use crate::{lower_module, Expr, Module, Stmt};
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower(src: String) -> Module {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache).unwrap();
            lower_module(&parsed.module, "test", "test.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

fn binding<'a>(module: &'a Module, name: &str) -> (&'a Type, &'a Expr) {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name: n,
                ty,
                init: Some(init),
                ..
            } if n == name => Some((ty, init)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}

#[test]
fn mapping_does_not_reuse_the_input_element_type() {
    for input in ["['x']", "[1]", "new Array<string>(1)"] {
        for method in ["map", "flatMap"] {
            let module = lower(format!(
                "function make(value: any): any {{ return {{ match() {{ return true; }} }}; }}\n\
                 const input = {input}; const output = input.{method}(value => make(value));"
            ));
            assert_eq!(
                binding(&module, "output").0,
                &Type::Array(Box::new(Type::Any)),
                "{input}.{method} must not inherit the receiver's element type"
            );
        }
    }
}

#[test]
fn element_preserving_methods_keep_the_input_type() {
    let module = lower(
        "const input = ['x']; const output = input.filter(value => true).slice().reverse();".into(),
    );
    assert_eq!(
        binding(&module, "output").0,
        &Type::Array(Box::new(Type::String))
    );
}

#[test]
fn lowered_callbacks_still_supply_precise_mapping_types() {
    let module = lower(
        "function lengthOf(value: string): number { return value.length; }\n\
         function lengthsOf(value: string): number[] { return [value.length]; }\n\
         const input = ['x'];\n\
         const mapped = input.map(lengthOf);\n\
         const flattened = input.flatMap(lengthsOf);"
            .into(),
    );
    for name in ["mapped", "flattened"] {
        assert_eq!(
            infer_expr_type(binding(&module, name).1, &HirTypeEnv::from_module(&module)),
            Type::Array(Box::new(Type::Number)),
            "{name} retains the lowered callback's return type: {:?}",
            binding(&module, name).1
        );
    }
}
