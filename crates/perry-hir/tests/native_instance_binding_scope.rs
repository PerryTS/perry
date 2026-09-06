//! #9847 — a native-instance tag created by a bare assignment
//! (`O = cp.spawn(...)`) must follow the BINDING it assigns to, not the
//! identifier's spelling.
//!
//! Before the fix, `lower_assign` registered the tag through
//! `push_module_native_instance`, whose key is the identifier text and whose
//! scope is the whole module. `cli_2.1.112.js` (claude-code) compiles as ONE
//! module containing `let O; try { O = fA1.spawn(...) }`, and `O` is a binding
//! 5,381 times in that file — so every `O` in the 13 MB program was typed
//! `child_process::Instance`, including the `for (let {segment: O} of ...)`
//! binding that holds a grapheme STRING in the hottest loop of a turn. Its
//! `O.codePointAt(0)` lowered as
//! `NativeMethodCall{module:"child_process", class_name:Some("Instance")}`
//! and reached the right answer only because native-instance dispatch falls
//! through to a generic path on a string receiver — once per grapheme.
//!
//! Note the ORDER dependency these fixtures encode deliberately: the tag can
//! only reach a function lowered AFTER the poisoning assignment, so `spawner`
//! precedes `widthLike` in every fixture below. With the order reversed the
//! defect does not reproduce and the test could not fail.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> perry_hir::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "native_instance_binding_scope.ts", &mut cache)
                    .expect("parse should succeed");
            lower_module(&parsed.module, "test", "native_instance_binding_scope.ts")
                .expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Debug-format just one function's body, so an assertion about `widthLike`
/// cannot be satisfied (or broken) by a node belonging to `spawner`.
fn body_of(module: &perry_hir::Module, name: &str) -> String {
    let func = module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            panic!(
                "function `{name}` not found; module has {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    format!("{:#?}", func.body)
}

/// The whole defect and both halves of the contract in one module: a
/// module-level handle read from a second function, a function-local handle,
/// and an unrelated binding that merely shares the local handle's spelling.
///
/// DO NOT REORDER `spawner` AND `widthLike` "for readability". The tag can only
/// reach a function lowered AFTER the assignment that creates it, so with
/// `widthLike` first the defect does not reproduce and
/// `a_same_named_binding_in_another_function_does_not_inherit_the_tag` passes
/// on the unfixed compiler — a test that cannot fail. This order was checked
/// against a pre-fix binary: `widthLike` lowered
/// `NativeMethodCall{module:"child_process", method:"codePointAt"}` there, and
/// the reversed order lowered the correct `PropertyGet`.
const FIXTURE: &str = r#"
import * as cp from "child_process";

let client: any;

export function init(): void {
  client = cp.spawn("true", []);
}

export function handler(): void {
  client.kill();
}

export function spawner(): any {
  let O: any;
  O = cp.spawn("false", []);
  O.kill();
  return O;
}

export function widthLike(q: any): number {
  let Y = 0;
  for (let { segment: O } of q) {
    Y += O.codePointAt(0) >= 4352 ? 2 : 1;
  }
  return Y;
}
"#;

#[test]
fn a_same_named_binding_in_another_function_does_not_inherit_the_tag() {
    let module = lower(FIXTURE);
    let width_like = body_of(&module, "widthLike");

    assert!(
        !width_like.contains("method: \"codePointAt\""),
        "the for-of `segment` binding holds a string and must not lower its \
         `.codePointAt` through child_process native-instance dispatch just \
         because an unrelated function spells its spawn handle `O` too: \
         {width_like}"
    );
    assert!(
        width_like.contains("property: \"codePointAt\""),
        "`O.codePointAt(0)` should lower as an ordinary property call: \
         {width_like}"
    );
}

#[test]
fn a_function_local_native_handle_still_dispatches_natively() {
    let module = lower(FIXTURE);
    let spawner = body_of(&module, "spawner");

    assert!(
        spawner.contains("method: \"kill\""),
        "the binding actually assigned from `cp.spawn(...)` must keep native \
         dispatch — scoping the tag to the binding must not lose the real \
         case: {spawner}"
    );
    assert!(
        spawner.contains("module: \"child_process\""),
        "and it must still be tagged as child_process: {spawner}"
    );
}

#[test]
fn a_module_level_handle_assigned_in_one_function_is_seen_in_another() {
    let module = lower(FIXTURE);
    let handler = body_of(&module, "handler");

    // This is what the module-wide table existed for: `client` is bound at
    // module level, assigned inside `init`, and read inside `handler`. Keyed
    // on the resolved binding it reaches just as far, because both functions
    // resolve `client` to the same LocalId.
    assert!(
        handler.contains("method: \"kill\"") && handler.contains("module: \"child_process\""),
        "a module-level handle assigned in another function must still \
         dispatch natively: {handler}"
    );
}

/// The issue's one-identifier A/B, as a test: two sources differing only in
/// whether the spawner's variable is spelled `O`. After the fix the two must
/// lower `widthLike` identically.
#[test]
fn renaming_the_spawner_variable_no_longer_changes_an_unrelated_function() {
    const ARM: &str = r#"
import * as cp from "child_process";

export function unrelatedSpawner(): any {
  let NAME: any;
  try { NAME = cp.spawn("true", []); } catch (M) { NAME = null; }
  return NAME;
}

export function widthLike(q: any): number {
  let Y = 0;
  for (let { segment: O } of q) {
    Y += O.codePointAt(0) >= 4352 ? 2 : 1;
  }
  return Y;
}
"#;

    // The rename is 3 characters longer, so every `byte_offset` in the file
    // shifts. Those are source positions, not lowering decisions — normalise
    // them, and nothing else, so the comparison is about the shape of the
    // lowered code. (Before the fix this comparison failed on the node itself:
    // `NativeMethodCall{module:"child_process", ...}` vs `Call{PropertyGet}`.)
    let normalise = |body: String| {
        body.split("byte_offset: ")
            .enumerate()
            .map(|(i, part)| {
                if i == 0 {
                    return part.to_string();
                }
                let rest = part.trim_start_matches(|c: char| c.is_ascii_digit());
                format!("byte_offset: N{rest}")
            })
            .collect::<String>()
    };

    let arm_a = normalise(body_of(&lower(&ARM.replace("NAME", "notO")), "widthLike"));
    let arm_b = normalise(body_of(&lower(&ARM.replace("NAME", "O")), "widthLike"));

    assert_eq!(
        arm_a, arm_b,
        "renaming a spawn handle in an unrelated function must not change how \
         `widthLike` lowers"
    );
    assert!(
        !arm_b.contains("method: \"codePointAt\""),
        "and neither arm may route the string read through native dispatch: \
         {arm_b}"
    );
}

const NEW_ASSIGNMENT_FIXTURE: &str = r#"
import { BlockList } from "net";

export function maker(): any {
  let O: any;
  O = new BlockList();
  O.addSubnet("10.0.0.0", 8);
  return O;
}

export function widthLike(q: any): number {
  let Y = 0;
  for (let { segment: O } of q) {
    Y += O.codePointAt(0) >= 4352 ? 2 : 1;
  }
  return Y;
}
"#;

const PROPAGATED_ASSIGNMENT_FIXTURE: &str = r#"
import * as cp from "child_process";

export function copier(): any {
  let source: any;
  source = cp.spawn("true", []);
  let O: any;
  O = source;
  O.kill();
  return O;
}

export function widthLike(q: any): number {
  let Y = 0;
  for (let { segment: O } of q) {
    Y += O.codePointAt(0) >= 4352 ? 2 : 1;
  }
  return Y;
}
"#;

fn assert_unrelated_width_binding_is_ordinary(module: &perry_hir::Module) {
    let width_like = body_of(module, "widthLike");
    assert!(
        !width_like.contains("method: \"codePointAt\""),
        "the unrelated for-of binding must not inherit a native-instance tag: \
         {width_like}"
    );
    assert!(
        width_like.contains("property: \"codePointAt\""),
        "the string binding's method should remain an ordinary property call: \
         {width_like}"
    );
}

#[test]
fn a_native_constructor_assignment_does_not_tag_an_unrelated_homonym() {
    let module = lower(NEW_ASSIGNMENT_FIXTURE);
    let maker = body_of(&module, "maker");
    assert!(
        maker.contains("module: \"net\"") && maker.contains("method: \"addSubnet\""),
        "the assigned BlockList binding must retain native dispatch: {maker}"
    );
    assert_unrelated_width_binding_is_ordinary(&module);
}

#[test]
fn a_propagated_native_assignment_does_not_tag_an_unrelated_homonym() {
    let module = lower(PROPAGATED_ASSIGNMENT_FIXTURE);
    let copier = body_of(&module, "copier");
    assert!(
        copier.contains("module: \"child_process\"") && copier.contains("method: \"kill\""),
        "the propagated handle binding must retain native dispatch: {copier}"
    );
    assert_unrelated_width_binding_is_ordinary(&module);
}
