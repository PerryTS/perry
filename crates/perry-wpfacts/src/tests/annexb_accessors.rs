//! Annex B installs use the same getter/setter flow as descriptor installs.
use super::*;

fn verdict(src: &str) -> Verdict {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert!(!sites.is_empty(), "missing witness site");
    assert_eq!(sites.len(), 1, "{sites:?}");
    sites[0].verdict.clone()
}

fn check(src: &str, proven: bool) {
    let v = verdict(src);
    assert_eq!(!matches!(v, Verdict::No(_)), proven, "{v:?}");
    assert!(
        !matches!(v, Verdict::No(ref s) if s.starts_with("dead site")),
        "{v:?}"
    );
}

#[test]
fn indexed_state_function_argument_stays_known() {
    check(
        r#"
        function enter(key: any, next: any) { return key; }
        class Machine { witness = 1; invoke: any = enter; }
        const machine: any = new Machine();
        const stateStack = [enter];
        function run(index: any) { stateStack[index]("k", enter); }
        run("0");
        function select(key: any) { machine.invoke = machine[key]; }
        select("invoke");
        machine.invoke("k", enter);
        console.log(machine.witness);
    "#,
        true,
    );
}

#[test]
fn getter_read_runs_effect() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = { cell, source: unknown() };
        o.__defineGetter__("k", function () { this.cell.value = this.source; return 0; });
        console.log(o.k);
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn setter_store_runs_effect() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = { cell };
        o.__defineSetter__("k", function (value: any) { this.cell.value = value; });
        o.k = unknown();
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn computed_install_key_runs_on_any_read() {
    check(
        r#"
        declare function unknown(): any;
        declare const key: string;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = {};
        o.__defineGetter__(key, function () { cell.value = unknown(); return 0; });
        console.log(o.other);
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn unknown_receiver_keeps_conservative_escape() {
    check(
        r#"import "unseen-host";

        declare const o: any;
        class Machine { witness = 1; }
        function enter(machine: any) { return machine.witness; }
        o.__defineGetter__("k", enter);
        console.log(enter(new Machine()));
    "#,
        false,
    );
}

#[test]
fn independently_handed_function_stays_unproven() {
    check(
        r#"
        declare function external(f: any): void;
        class Machine { witness = 1; }
        function enter(machine: any) { return machine.witness; }
        const o: any = {};
        o.__defineGetter__("k", enter);
        external(enter);
        console.log(enter(new Machine()));
    "#,
        false,
    );
}

#[test]
fn noncallable_installs_nothing() {
    check(
        r#"
        class Machine { witness = 1; }
        const machine = new Machine();
        const o: any = {};
        o.__defineGetter__("k", machine);
        console.log(machine.witness);
    "#,
        true,
    );
}

#[test]
fn short_arity_installs_nothing() {
    check(
        r#"
        class Machine { witness = 1; }
        const o: any = new Machine();
        o.__defineGetter__("k");
        console.log(o.witness);
    "#,
        true,
    );
}

#[test]
fn named_getter_result_flows_to_the_read() {
    check(
        r#"
        class Machine { witness = 1; }
        const o: any = {};
        o.__defineGetter__("k", function () { return new Machine(); });
        console.log(o.k.witness);
    "#,
        true,
    );
}

#[test]
fn computed_getter_result_flows_to_the_read() {
    check(
        r#"
        declare const key: string;
        class Machine { witness = 1; }
        const o: any = {};
        o.__defineGetter__(key, function () { return new Machine(); });
        console.log(o.k.witness);
    "#,
        true,
    );
}

#[test]
fn mixed_callable_and_noncallable_installs_functions_only() {
    check(
        r#"import "unseen-host";

        declare const choose: boolean;
        declare function unknown(): any;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = {};
        const getter = choose ? function () { cell.value = unknown(); return 0; } : 1;
        o.__defineGetter__("k", getter);
        console.log(o.k);
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn function_receiver_uses_accessor_model() {
    check(
        r#"
        class Machine { witness = 1; }
        function o() {}
        o.__defineGetter__("k", function () { return new Machine(); });
        console.log(o.k.witness);
    "#,
        true,
    );
}

#[test]
fn array_receiver_uses_accessor_model() {
    check(
        r#"
        class Machine { witness = 1; }
        const o: any = [];
        o.__defineGetter__("k", function () { return new Machine(); });
        console.log(o.k.witness);
    "#,
        true,
    );
}

#[test]
fn numeric_constant_key_preserves_other_fields() {
    check(
        r#"
        class Machine { witness = 1; }
        const o: any = new Machine();
        o.__defineGetter__(1, function () { return 0; });
        console.log(o.witness);
    "#,
        true,
    );
}

#[test]
fn getter_omitted_argument_runs_default() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = {};
        o.__defineGetter__("k", function (value = unknown()) { cell.value = value; });
        console.log(o.k);
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn setter_omitted_arguments_run_defaults() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { value: any = { witness: 1 }; }
        const cell = new Cell();
        const o: any = {};
        o.__defineSetter__("k", function (value: any, other = unknown()) { cell.value = other; });
        o.k = 0;
        console.log(cell.value.witness);
    "#,
        false,
    );
}

#[test]
fn callable_builtin_accessor_is_not_a_data_field() {
    check(
        r#"
        const o: any = { witness: 1 };
        o.__defineGetter__("witness", Object);
        console.log(o.witness);
    "#,
        false,
    );
}

#[test]
fn class_accessor_is_not_a_data_field() {
    check(
        r#"
        class C {}
        const o: any = { witness: 1 };
        o.__defineGetter__("witness", C);
        console.log(o.witness);
    "#,
        false,
    );
}

#[test]
fn noncallable_namespace_installs_nothing() {
    check(
        r#"
        const o: any = { witness: 1 };
        o.__defineGetter__("k", Math);
        console.log(o.witness);
    "#,
        true,
    );
}

#[test]
fn spread_prefix_does_not_drop_the_accessor_function() {
    check(
        r#"
        const key = ["witness"];
        const o: any = { witness: 1 };
        o.__defineGetter__(...key, () => 2);
        console.log(o.witness);
    "#,
        false,
    );
}

#[test]
fn native_function_computed_receiver_keeps_conservative_path() {
    // Computed native keys are outside the ordinary program-field predicate.
    check(
        r#"
        declare const key: string;
        const o: any = Object.assign;
        Object.defineProperty(o, key, { get() { return { witness: 1 }; } });
        console.log(o.other.witness);
    "#,
        false,
    );
}

#[test]
fn native_function_receiver_uses_accessor_result() {
    check(
        r#"
        const o: any = Object.assign;
        Object.defineProperty(o, "k", { get() { return { witness: 1 }; } });
        console.log(o.k.witness);
    "#,
        true,
    );
}

#[test]
fn calls_without_an_installer_create_no_accessor_work() {
    let src = r#"
        function add(a: any, b: any) { return a + b; }
        class Cell { witness = 1; run(a: any, b: any) { return add(a, b); } }
        const cell = new Cell();
        const box = { run: add };
        const stack = [add];
        function invoke(index: any) { return stack[index](1, 2); }
        cell.run(1, 2); cell["run"](3, 4);
        box.run(1, 2); box["run"](3, 4); invoke(0);
        console.log(cell.witness);
    "#;
    check(src, true);
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    assert!(g.m.acc_groups.is_empty());
    assert!(g.m.walk_nodes.is_empty());
    assert!(g.m.accessors.is_empty());
    assert!(g.m.nodes.iter().all(|n| n.cons.iter().all(|c| !matches!(
        c,
        crate::model::Cons::AccessorInstall { .. }
            | crate::model::Cons::Accessor { .. }
            | crate::model::Cons::HolderStep { .. }
            | crate::model::Cons::VisitHolders { .. }
            | crate::model::Cons::VisitStop { .. }
            | crate::model::Cons::AnyAccessors { .. }
    ))));
}

#[test]
fn a_late_install_replays_prior_inherited_reads() {
    for key in ["\"k\"", "key"] {
        let setup = r#"
            declare function unknown(): any;
            declare const key: string;
            class Cell { value: any = { witness: 1 }; }
            const cell = new Cell();
            const holder: any = { cell, source: unknown() };
            const o: any = Object.create(holder);
            function provide() {
                return function () { this.cell.value = this.source; return 0; };
            }
        "#;
        let install = format!("holder.__defineGetter__({key}, provide());");
        let read = "console.log(o.k);";
        let tail = "console.log(cell.value.witness);";
        let early = verdict(&format!("{setup}{install}{read}{tail}"));
        let late = verdict(&format!("{setup}{read}{install}{tail}"));
        assert!(
            matches!(early, Verdict::No(ref why) if !why.starts_with("dead site")),
            "{early:?}"
        );
        assert_eq!(
            late, early,
            "key={key}: prior reads must observe the installed getter"
        );
    }
}

#[test]
fn an_installed_native_namespace_member_call_runs_the_getter_result() {
    let setup = r#"
        declare function unknown(value: any): void;
        class Cell { witness = 1; }
        const o = new Cell();
        function g() { unknown(o); }
    "#;
    let call = "(Object as any).zap(o); console.log(o.witness);";
    check(&format!("{setup}{call}"), true);
    check(
        &format!(
            "{setup}(Object as any).__defineGetter__(\"zap\", () => g); console.log(o.witness);"
        ),
        true,
    );
    check(
        &format!("{setup}(Object as any).__defineGetter__(\"zap\", () => g);{call}"),
        false,
    );
}

/// `Object.prototype.__defineSetter__` installs on `Object.prototype` as
/// `defineProperty` does (E23): the setter a store on a literal runs
/// writes a string over the literal's field.
#[test]
fn object_prototype_setter_install_runs_on_store() {
    check(
        r#"
        class V { n = 1; witness(): number { return this.n; } }
        (Object.prototype as any).__defineSetter__("constructor", function (this: any, v: any) {
            this.a = "s";
        });
        const o: any = { a: new V() };
        o.constructor = 5;
        console.log(o.a.witness());
    "#,
        false,
    );
}

/// The getter form, run by a read through a literal.
#[test]
fn object_prototype_getter_install_runs_on_read() {
    check(
        r#"
        class V { n = 1; witness(): number { return this.n; } }
        const o: any = { a: new V() };
        (Object.prototype as any).__defineGetter__("constructor", function (this: any) {
            this.a = "s";
            return 1;
        });
        o.constructor;
        console.log(o.a.witness());
    "#,
        false,
    );
}

/// The solved model of `src` (the pipeline `compute` runs).
fn solved(src: &str) -> crate::gen::Gen<'static> {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g
}

fn site_verdicts(src: &str, key: &str) -> Vec<String> {
    let pf = facts(src);
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    s.sort_by_key(|s| s.byte_offset);
    assert!(!s.is_empty(), "no site {key}");
    s.iter().map(|s| format!("{:?}", s.verdict)).collect()
}

/// SKINACC2 (skin seq 0/1): a descriptor query on a builtin function walks
/// the one field object that holds the own fields of every builtin function
/// (`Shared::BuiltinFn`). Its answer is the builtin functions declared
/// accessors (`Family::Function` and the realm accessors the key may name),
/// never an unknown value: the getter and setter found here are called with
/// `o`, and `o.m()` stays the program body; the realm is not held.
#[test]
fn skinacc2_builtin_fn_descriptor_lookup_adds_no_unknown() {
    for key in ["k", "", "true"] {
        let src = format!(
            "declare const s: string;\nconst o = {{ m() {{ return 1; }} }};\nconst k = JSON.parse(s)[0];\nconst z: any = (Math as any)[k]; z();\n\
             const f: any = Object.prototype.toLocaleString;\n\
             const g = f.__lookupSetter__({key}); const h = f.__lookupGetter__({key});\n\
             if (g) g.call(o); if (h) h.call(o);\nconst r = o.m();\n"
        );
        let v = site_verdicts(&src, "m");
        assert!(v.iter().all(|x| x.starts_with("T0Mono")), "key {key:?}: the lookup on the builtin fn added an unknown value (accessor_lookup.rs:198/:264): {v:?}");
        assert!(!solved(&src).m.realm_held, "key {key:?}: realm held");
    }
}

/// The soundness side: builtin function objects that own accessors (the
/// runtime installs `get [Symbol.species]` on Array, ArrayBuffer, Map, Set,
/// Promise, RegExp, SharedArrayBuffer, %TypedArray%: install_static.rs
/// `install_builtin_species_accessor`; `caller`/`arguments` on
/// %Function.prototype%: populate.rs) answer with that accessor function:
/// a field read of the result is reached (never only `undefined`, which
/// would make the read a dead site).
#[test]
fn skinacc2_builtin_fn_own_accessors_answer_their_function() {
    for q in [
        "Array.__lookupGetter__(Symbol.species)",
        "RegExp.__lookupGetter__(k)",
        "Function.prototype.__lookupGetter__(\"caller\")",
        "(Object.prototype.toLocaleString as any).__lookupGetter__(k)",
    ] {
        let src = format!(
            "declare const s: string;\nconst k = JSON.parse(s)[0];\nconst a: any = {q};\nconsole.log(a.wq);\n"
        );
        let v = site_verdicts(&src, "wq");
        assert!(v.iter().all(|x| !x.contains("dead site") && !x.starts_with("Dead")), "{q}: the accessor function is missing from the answer: {v:?}");
    }
}
