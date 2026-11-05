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
        r#"
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
        r#"
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
