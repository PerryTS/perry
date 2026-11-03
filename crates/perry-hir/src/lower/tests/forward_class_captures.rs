//! A class member (method, accessor, constructor, instance field initializer)
//! that names a later class declaration of the same scope resolves that name
//! statically (`ClassRef`), exactly as it did before forward class bindings
//! existed, instead of capturing the later class's still uninitialized
//! binding.
//!
//! An earlier closure naming the later class (`const read = () => C`) is what
//! pre-registers the binding as a local, so that the closure reads it live.
//! The member bodies have to keep their own reads off that local: a
//! `RegisterClassCaptures` snapshot taken at the class declaration would hold
//! `undefined`, and every later way of repairing the snapshot (refreshing it
//! once the later class initializes) misses some shape of evaluation: an
//! inner block that has left scope by then, several evaluations of the same
//! declaration, instances created before the later class initializes.
//!
//! The scoping is exact: only the member bodies of a class of the function
//! that owns the pending binding are affected. A closure made while the class
//! is defined (a computed key, the heritage, a static initializer) is code of
//! the enclosing function and reads the live binding; a catch clause or a
//! block is no other function; a pending binding of the same name in an
//! enclosing scope or function is hidden together with the inner one, never
//! exposed by it.
//!
//! The tests below read the lowered HIR for what the program can observe:
//! what each class captures, and what its methods read the later class
//! through.

fn lower(source: &str) -> crate::ir::Module {
    let ast = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&ast, "t", "t.ts").expect("source lowers")
}

fn strip(debug: String) -> String {
    debug.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn main_body(module: &crate::ir::Module) -> String {
    let function = module
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("fixture declares function main");
    strip(format!("{:?}", function.body))
}

fn class_text(module: &crate::ir::Module, name: &str) -> String {
    let class = module
        .classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("fixture declares class {name}"));
    strip(format!("{class:?}"))
}

/// The local id `Let{id:N,name:"<name>"` declares, first occurrence.
fn local_id(body: &str, name: &str) -> String {
    let marker = format!(",name:\"{name}\"");
    let at = body
        .find(&marker)
        .unwrap_or_else(|| panic!("`{name}` is declared in the body: {body}"));
    let digits: String = body[..at]
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.chars().rev().collect()
}

/// Every capture list `RegisterClassCaptures` records for `class`.
fn registered_captures(body: &str, class: &str) -> Vec<String> {
    let marker = format!("RegisterClassCaptures{{class_name:\"{class}\",captures:[");
    body.match_indices(&marker)
        .map(|(at, _)| {
            let rest = &body[at + marker.len()..];
            rest[..rest.find(']').expect("capture list ends")].to_string()
        })
        .collect()
}

/// `Holder` reads the later class `C` through the static name, captures
/// nothing, and is an unowned evaluation that nothing refreshes.
fn assert_holder_reads_c_statically(source: &str) {
    let module = lower(source);
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    assert!(
        holder.contains("Return(Some(ClassRef(\"C\")))"),
        "Holder reads C through the static class name: {holder}"
    );
    assert!(
        !holder.contains("ClassEnvGet"),
        "Holder holds no capture of the uninitialized C: {holder}"
    );
    assert!(
        registered_captures(&body, "Holder").is_empty(),
        "Holder registers no captures: {body}"
    );
    assert!(
        body.contains("evaluation_owner:None") && !body.contains("evaluation_owner:Some("),
        "nothing waits on C to initialize Holder: {body}"
    );
    assert!(
        !body.contains("RefreshClassExprCaptures{"),
        "nothing refreshes Holder: {body}"
    );
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("captures:[{c}]")),
        "the earlier closure still captures C's own binding live: {body}"
    );
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C's binding is a pre-allocated box the closure reads: {body}"
    );
}

#[test]
fn loop_body_class_naming_a_later_class_reads_it_statically() {
    assert_holder_reads_c_statically(
        "function main() { const out = []; for (let i = 0; i < 2; i++) { \
         const read = () => C; class Holder { value() { return C; } } class C {} \
         out.push([new Holder().value() === C, read() === C]); } return out; }",
    );
}

#[test]
fn nested_block_class_naming_a_later_class_reads_it_statically() {
    assert_holder_reads_c_statically(
        "function main() { { const read = () => C; class Holder { value() { return C; } } \
         class C {} return [new Holder().value() === C, read() === C]; } }",
    );
}

#[test]
fn class_in_an_inner_block_keeps_its_own_captures_and_reads_the_later_class_statically() {
    // `x` has left scope by the time `C` initializes: no refresh at `C` can
    // read it, so the class must not depend on one.
    let module = lower(
        "function main() { for (let i = 0; i < 1; i++) { \
         const read = () => C, classes = []; \
         { const x = i; class Holder { value() { return [x, C]; } } classes.push(Holder); } \
         class C {} const H = classes[0]; \
         return [new H().value()[1] === C, read() === C]; } }",
    );
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    let x = local_id(&body, "x");
    assert_eq!(
        registered_captures(&body, "Holder"),
        vec![format!("LocalGet({x})")],
        "Holder captures `x` and nothing else: {body}"
    );
    assert!(
        holder.contains("ClassRef(\"C\")") && holder.contains("ClassEnvGet"),
        "Holder reads `x` from its captures and C through the static name: {holder}"
    );
    assert!(
        !body.contains("RefreshClassExprCaptures{") && !body.contains("evaluation_owner:Some("),
        "nothing refreshes Holder: {body}"
    );
}

#[test]
fn every_evaluation_of_a_class_in_an_inner_loop_reads_the_later_class_statically() {
    assert_holder_reads_c_statically(
        "function main() { for (let outer = 0; outer < 1; outer++) { \
         const read = () => C, classes = []; \
         for (let j = 0; j < 2; j++) { class Holder { value() { return C; } } classes.push(Holder); } \
         class C {} const A = classes[0], B = classes[1]; \
         return [new A().value() === C, new B().value() === C]; } }",
    );
}

#[test]
fn class_with_an_evaluated_parent_reads_the_later_class_statically_for_early_instances() {
    // `new Holder()` runs before `C` initializes; an instance would carry a
    // copy of any captured binding made at that point.
    let source = "function main() { for (let i = 0; i < 1; i++) { \
         const read = () => C; class Holder extends Error { value() { return C; } } \
         const early = new Holder(); class C {} \
         return [early.value() === C, read() === C]; } }";
    assert_holder_reads_c_statically(source);
    let module = lower(source);
    let holder = class_text(&module, "Holder");
    assert!(
        !holder.contains("__perry_cap_"),
        "instances of Holder carry no capture fields: {holder}"
    );
}

#[test]
fn class_naming_an_earlier_class_still_captures_it() {
    let module = lower(
        "function main() { const out = []; for (let i = 0; i < 2; i++) { \
         class C {} const read = () => C; class Holder { value() { return C; } } \
         out.push([new Holder().value() === C, read() === C]); } return out; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert_eq!(
        registered_captures(&body, "Holder"),
        vec![format!("LocalGet({c})")],
        "a backward reference is initialized when the capture is taken: {body}"
    );
    assert!(
        !body.contains("RefreshClassExprCaptures{"),
        "nothing needs refreshing: {body}"
    );
}

#[test]
fn class_in_a_nested_closure_naming_a_later_class_of_the_outer_function_captures_it_live() {
    // The closure runs after `C` initialized, and reads the same box the
    // earlier closure does.
    let module = lower(
        "function main() { const read = () => C; \
         const make = () => { class Holder { value() { return C; } } return Holder; }; \
         class C {} return [make(), read()]; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert_eq!(
        registered_captures(&body, "Holder")
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([format!("LocalGet({c})")]),
        "the class captures the box-backed C: {body}"
    );
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C is a pre-allocated box: {body}"
    );
}

#[test]
fn generator_with_a_parameter_default_naming_a_later_class_captures_it() {
    // The default is evaluated by the generator's own call, in its
    // activation, so the generator must be a closure over C's binding.
    for default in ["value = C", "{ a: value = C } = {}", "value = () => C"] {
        let module = lower(&format!(
            "function main() {{ for (let i = 0; i < 1; i++) {{ \
             function* read({default}) {{ yield value; }} class C {{}} return read; }} }}"
        ));
        let body = main_body(&module);
        let c = local_id(&body, "C");
        assert!(
            body.contains("is_generator:true") && body.contains(&format!("captures:[{c}]")),
            "generator with default `{default}` must be a closure capturing C: {body}"
        );
        assert!(
            !body.contains("FuncRef("),
            "generator with default `{default}` must not take the captureless path: {body}"
        );
    }
}

#[test]
fn generator_without_outer_references_stays_captureless() {
    let module = lower(
        "function main() { for (let i = 0; i < 1; i++) { \
         function* read(value = 1) { yield value; } class C {} return read; } }",
    );
    let body = main_body(&module);
    assert!(body.contains("FuncRef("), "{body}");
}

/// `Holder` holds a closure that was created while the class definition was
/// evaluated (a computed key, the heritage, a static initializer) and that
/// reads the later class `C`: it is a closure over C's own binding, so it
/// observes the evaluated class (`LocalGet`), not the class template.
fn assert_definition_time_closure_reads_c_live(source: &str) {
    let module = lower(source);
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C's binding is a pre-allocated box: {body}"
    );
    let live = format!("body:[Return(Some(LocalGet({c})))],captures:[{c}]");
    assert!(
        body.contains(&live) || holder.contains(&live),
        "the closure made while Holder is defined reads C's box: {body} {holder}"
    );
    assert!(
        !holder.contains("ClassEnvGet") && registered_captures(&body, "Holder").is_empty(),
        "Holder itself captures nothing: {holder} {body}"
    );
}

#[test]
fn closure_in_a_computed_key_reads_the_later_class_live() {
    let source = "function main() { let read; \
         const Holder = class { [(read = () => C, \"value\")]() { return C; } }; \
         class C extends Error {} return [read() === C, new Holder().value() === C]; }";
    assert_definition_time_closure_reads_c_live(source);
    let holder = class_text(&lower(source), "Holder");
    assert!(
        holder.contains("Return(Some(ClassRef(\"C\")))"),
        "the member body still reads C through the static name: {holder}"
    );
}

#[test]
fn closure_in_a_heritage_expression_reads_the_later_class_live() {
    assert_definition_time_closure_reads_c_live(
        "function main() { let read; \
         class Holder extends (read = () => C, Object) { value() { return C; } } \
         class C extends Error {} return [read() === C, new Holder().value() === C]; }",
    );
}

#[test]
fn closure_in_a_static_initializer_reads_the_later_class_live() {
    assert_definition_time_closure_reads_c_live(
        "function main() { let read; \
         class Holder { static seed = (read = () => C, 1); value() { return C; } } \
         class C extends Error {} return [read() === C, new Holder().value() === C]; }",
    );
}

#[test]
fn closure_in_a_class_declarations_computed_key_reads_a_capturing_later_class_live() {
    // `C` captures the parameter `n`, so it is a per-evaluation class value:
    // the closure made while `Holder` is declared must read that value.
    let source = "function main(n) { let read; \
         class Holder { [(read = () => C, \"value\")]() {} } \
         class C { value() { return n; } } return read() === C; } main(1);";
    assert_definition_time_closure_reads_c_live(source);
}

#[test]
fn closure_in_a_class_declarations_computed_key_reads_a_plain_later_class_live() {
    // Whether `C` is evaluated is not predicted: a plain class is registered
    // like any other later binding, and its declaration initializes the box
    // with the template, which is the class itself.
    assert_definition_time_closure_reads_c_live(
        "function main() { let read; \
         class Holder { [(read = () => C, \"value\")]() {} } \
         class C {} return read() === C; }",
    );
}

#[test]
fn subclass_of_a_capturing_class_read_by_an_earlier_closure_is_registered() {
    // `Base` captures `n`, which makes `Base`, and with it `Derived`, a
    // per-evaluation class value. `Derived` has no member naming it and no
    // syntax of its own that says so.
    let module = lower(
        "function main(n) { const read = () => Derived; \
         class Base { value() { return n; } } class Derived extends Base {} \
         return read() === Derived; } main(7);",
    );
    let body = main_body(&module);
    let derived = local_id(&body, "Derived");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{derived}])")),
        "Derived's binding is a pre-allocated box the closure reads: {body}"
    );
    assert!(
        body.contains(&format!(
            "body:[Return(Some(LocalGet({derived})))],captures:[{derived}]"
        )),
        "the earlier closure reads Derived's box: {body}"
    );
}

#[test]
fn plain_later_class_read_by_an_earlier_closure_is_registered() {
    let module = lower("function main() { const read = () => C; class C {} return read() === C; }");
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])"))
            && body.contains(&format!(
                "body:[Return(Some(LocalGet({c})))],captures:[{c}]"
            )),
        "the closure reads C's box: {body}"
    );
}

#[test]
fn later_class_read_only_by_a_member_body_is_not_registered() {
    // A member names the later class statically (it is hidden while the
    // member lowers), so nothing reads a binding and there is nothing to
    // box.
    let module = lower(
        "function main() { class Holder { value() { return C; } } class C {} \
         return new Holder().value() === C; }",
    );
    let body = main_body(&module);
    assert!(!body.contains("PreallocateTdzBoxes"), "{body}");
}

#[test]
fn closure_escaping_a_member_body_keeps_reading_the_template_as_before() {
    // Definition-time closures read the live binding; a closure created
    // inside a member body is part of that body, which has always named
    // the later class through the static name.
    let module = lower(
        "function main() { for (let i = 0; i < 1; i++) { \
         const read = () => C; \
         class Holder { make() { return () => C; } } \
         class C {} return [new Holder().make()(), read()]; } }",
    );
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    assert!(
        holder.contains("body:[Return(Some(ClassRef(\"C\")))],captures:[]"),
        "the escaping closure names C through the static name: {holder}"
    );
    assert!(
        registered_captures(&body, "Holder").is_empty() && !holder.contains("ClassEnvGet"),
        "Holder captures nothing: {holder} {body}"
    );
}

#[test]
fn class_in_a_catch_clause_reads_the_later_class_statically() {
    // A catch clause is a scope of the same function: it must not make the
    // class look like it belongs to another function than `C`'s.
    assert_holder_reads_c_statically(
        "function main() { for (let i = 0; i < 1; i++) { try { throw 0; } catch (e) { \
         const read = () => C; class Holder { value() { return C; } } class C {} \
         return [new Holder().value() === C, read() === C]; } } }",
    );
}

#[test]
fn class_in_a_nested_catch_clause_reads_the_later_class_statically() {
    // Same, with the class declared in a block inside the catch clause.
    assert_holder_reads_c_statically(
        "function main() { for (let i = 0; i < 1; i++) { try { throw 0; } catch (e) { { \
         const read = () => C; class Holder { value() { return C; } } class C {} \
         return [new Holder().value() === C, read() === C]; } } } }",
    );
}

#[test]
fn shadowed_pending_class_resolves_to_the_inner_class_statically() {
    // Two pending bindings named `C`: Holder belongs to the inner block, so
    // it reads the inner class, and must not capture the outer binding that
    // hiding the inner one would otherwise expose.
    let module = lower(
        "class C {} function main() { for (let i = 0; i < 1; i++) { \
         const outerRead = () => C; \
         { const innerRead = () => C; class Holder { value() { return C; } } class C {} \
         return [new Holder().value() === C, innerRead() === C]; } \
         class C {} return outerRead() === C; } }",
    );
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    assert!(
        holder.contains("Return(Some(ClassRef(\"C$1\")))"),
        "Holder reads the inner class through its own template name: {holder}"
    );
    assert!(
        !holder.contains("ClassEnvGet") && registered_captures(&body, "Holder").is_empty(),
        "Holder captures neither pending C: {holder} {body}"
    );
    assert!(
        body.contains("PreallocateTdzBoxes([0])") && body.contains("PreallocateTdzBoxes([1])"),
        "both pending bindings stay boxes the earlier closures read: {body}"
    );
}

#[test]
fn pending_class_of_an_enclosing_function_with_the_same_name_is_not_exposed() {
    // `make` is its own function: its `C` is pending there while the outer
    // function's `C` is pending too. The class in `make` reads its own.
    let module = lower(
        "function main() { const outerRead = () => C; \
         const make = () => { const innerRead = () => C; \
         class Holder { value() { return C; } } class C {} return [Holder, innerRead]; }; \
         class C {} return [make(), outerRead()]; }",
    );
    let body = main_body(&module);
    let holder = class_text(&module, "Holder");
    assert!(
        holder.contains("Return(Some(ClassRef(\"C$1\")))")
            || holder.contains("Return(Some(ClassRef(\"C\")))"),
        "Holder reads a static class name: {holder}"
    );
    assert!(
        !holder.contains("ClassEnvGet") && registered_captures(&body, "Holder").is_empty(),
        "Holder captures no pending C: {holder} {body}"
    );
}

#[test]
fn closure_with_a_constant_direct_eval_reads_a_later_class_live() {
    // The scan cannot read the string, and the constant source lowers into a
    // real read of `C` later: a direct eval in a closure counts as a read of
    // every later class of the scope.
    let module = lower(
        "function main() { const read = () => eval(\"C\"); class C extends Error {} \
         return read() === C; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C's binding is a pre-allocated box the eval's read uses: {body}"
    );
    assert!(
        !body.contains("ClassRef(\"C\")"),
        "the closure does not read the template: {body}"
    );
}

#[test]
fn direct_eval_of_a_runtime_string_in_a_closure_registers_a_later_class() {
    let module = lower(
        "function main() { const read = (source) => eval(source); class C extends Error {} \
         return read(\"C\") === C; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C's binding is a pre-allocated box the eval's bindings read: {body}"
    );
}

#[test]
fn direct_eval_in_a_member_body_lowers_while_a_pending_class_is_hidden() {
    // The hidden binding's name is not an identifier: the bindings object of
    // the eval must not spell it.
    let module = lower(
        "function main() { const read = () => C; \
         class Holder { static evaluate(source) { return eval(source); } } class C {} \
         return [Holder.evaluate(\"1\"), read() === C]; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C stays a pending binding for the earlier closure: {body}"
    );
}

#[test]
fn direct_eval_in_the_function_body_registers_a_later_class() {
    // Nothing in the scope is a closure yet, but the eval creates one that
    // outlives the call: it reads the later class through the binding.
    let module = lower(
        "function main() { const read = eval(\"() => C\"); class C extends Error {} \
         return read() === C; }",
    );
    let body = main_body(&module);
    let c = local_id(&body, "C");
    assert!(
        body.contains(&format!("PreallocateTdzBoxes([{c}])")),
        "C's binding is a pre-allocated box the eval's closure reads: {body}"
    );
    assert!(
        !body.contains("ClassRef(\"C\")"),
        "the closure does not read the template: {body}"
    );
}

#[test]
fn direct_eval_in_the_function_body_registers_only_the_classes_after_it() {
    // A class declared before the eval is initialized by then: no earlier
    // closure can read it, so it stays an ordinary class.
    let module = lower("function main() { class C {} return eval(\"C\") === C; }");
    let body = main_body(&module);
    assert!(
        !body.contains("PreallocateTdzBoxes"),
        "a class declared before the eval is not pre-registered: {body}"
    );
}
