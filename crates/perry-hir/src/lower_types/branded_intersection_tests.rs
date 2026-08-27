//! Branded-primitive intersections (`number & { __tag: T }`) lower to the
//! primitive they spell, so an id alias gets the same guarded native
//! treatment as a plain `number` annotation. Object-object merges and
//! conflicting primitives keep the `Any` lowering.

#![cfg(test)]

use crate::lower_module;
use crate::types::Type;
use crate::Module;
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn first_param_type(module: &Module, func: &str) -> Type {
    module
        .functions
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func} not lowered"))
        .params[0]
        .ty
        .clone()
}

#[test]
fn branded_number_and_string_intersections_lower_to_their_primitive() {
    let module = lower_src(
        r#"
declare const __tag: unique symbol;
type Id = number & { readonly [__tag]?: "id" };
type Name = string & { readonly __brand: "name" };
type Loose = string & {};
type Both = number & { a: 1 } & { b: 2 };
export function fId(x: Id): boolean { return x >= 0; }
export function fName(x: Name): boolean { return x === "a"; }
export function fLoose(x: Loose): boolean { return x === "a"; }
export function fBoth(x: Both): boolean { return x >= 0; }
"#,
    );
    assert_eq!(first_param_type(&module, "fId"), Type::Number);
    assert_eq!(first_param_type(&module, "fName"), Type::String);
    assert_eq!(first_param_type(&module, "fLoose"), Type::String);
    assert_eq!(first_param_type(&module, "fBoth"), Type::Number);
}

#[test]
fn non_branded_intersections_stay_dynamic() {
    let module = lower_src(
        r#"
type A = { a: number };
type B = { b: string };
type Merged = A & B;
type Conflict = number & string;
type Arr = number[] & { extra: true };
export function fMerged(x: Merged): number { return x.a; }
export function fConflict(x: Conflict): boolean { return x === 1; }
export function fArr(x: Arr): number { return x.length; }
"#,
    );
    assert_eq!(first_param_type(&module, "fMerged"), Type::Any);
    assert_eq!(first_param_type(&module, "fConflict"), Type::Any);
    assert_eq!(first_param_type(&module, "fArr"), Type::Any);
}

#[test]
fn generic_branded_alias_reference_stays_generic_until_the_driver_pass() {
    // The same-module resolver only settles non-generic aliases; a generic
    // instantiation is closed later by `type_alias_resolve` with the alias
    // table the driver assembles across modules.
    let module = lower_src(
        r#"
declare const __tag: unique symbol;
export type EntityId<T = unknown> = number & { readonly [__tag]?: T };
export function f(x: EntityId<string>): boolean { return x >= 0; }
"#,
    );
    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "EntityId")
        .expect("alias recorded");
    assert_eq!(alias.ty, Type::Number);
    assert_eq!(
        first_param_type(&module, "f"),
        Type::Generic {
            base: "EntityId".to_string(),
            type_args: vec![Type::String],
        }
    );
}
