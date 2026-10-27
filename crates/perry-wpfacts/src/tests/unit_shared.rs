//! Per-unit mode: the runtime's own objects (`Object.prototype`, the builtin
//! namespaces and prototypes, builtin function values) are one object for
//! the whole program. A value contract names them; given to unknown code
//! any other way (the other units' code included) they are handed over, and
//! every unit's store into ⊤ may land in them. What one unit stores or
//! installs on them is replayed in every unit's solve (`model::SharedWrite`),
//! so the per-unit facts never claim more than whole-program facts about the
//! objects such a write can reach, and a contract that names them costs the
//! other units nothing.

use super::*;

fn hir(src: &str) -> perry_hir::Module {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    perry_hir::lower_module(&module, "t", "t.ts").expect("lowers")
}

fn units(src: &str) -> ProgramFacts {
    let h = hir(src);
    crate::compute_units(&[("t.ts".to_string(), &h)], &FactsConfig::default())
}

fn whole(src: &str) -> ProgramFacts {
    let h = hir(src);
    crate::compute(&[("t.ts".to_string(), &h)], &FactsConfig::default())
}

/// Template claims the per-unit run makes and the whole-program run does
/// not. Per unit, every unit declaring a literal class has its own copy,
/// describing its own instances; what the program relies on is their meet
/// (`ProgramFacts::stable_classes`), so the copies are met first.
fn templates_stronger(u: &ProgramFacts, w: &ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for tw in &w.templates {
        let copies: Vec<_> = u
            .templates
            .iter()
            .filter(|t| t.display == tw.display)
            .collect();
        if copies.is_empty() {
            continue;
        }
        let d = &tw.display;
        if copies.iter().all(|t| t.prefix_stable) && !tw.prefix_stable {
            out.push(format!("{d} prefix_stable"));
        }
        if copies.iter().all(|t| t.escape.is_none()) && tw.escape.is_some() {
            out.push(format!("{d} escape {:?}", tw.escape));
        }
        for fw in &tw.fields {
            let fs: Vec<_> = copies
                .iter()
                .filter_map(|t| t.fields.iter().find(|f| f.key == fw.key))
                .collect();
            if fs.is_empty() {
                continue;
            }
            if fs.iter().all(|f| f.readonly) && !fw.readonly {
                out.push(format!("{d}.{} readonly", fw.key));
            }
            let mu = fs.iter().fold(0u16, |m, f| m | f.kind.mask());
            if mu & fw.kind.mask() != fw.kind.mask() {
                out.push(format!(
                    "{d}.{} kind {mu:#x} vs {:#x}",
                    fw.key,
                    fw.kind.mask()
                ));
            }
        }
    }
    out
}

/// Template claims the whole-program run makes for the one literal with the
/// fields `keys` (and the keys its lookups add) about those fields that the
/// per-unit run does not (its copies met): a precision test expects none.
fn literal_weaker(u: &ProgramFacts, w: &ProgramFacts, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = 0;
    for tw in &w.templates {
        if !keys.iter().all(|k| tw.fields.iter().any(|f| f.key == *k)) {
            continue;
        }
        seen += 1;
        let copies: Vec<_> = u
            .templates
            .iter()
            .filter(|t| t.display == tw.display)
            .collect();
        let d = &tw.display;
        assert!(!copies.is_empty(), "no per-unit copy of {d}");
        if tw.prefix_stable && !copies.iter().all(|t| t.prefix_stable) {
            out.push(format!("{d} prefix_stable"));
        }
        if tw.escape.is_none() && copies.iter().any(|t| t.escape.is_some()) {
            out.push(format!("{d} escape"));
        }
        for fw in tw.fields.iter().filter(|f| keys.contains(&f.key.as_str())) {
            let fs: Vec<_> = copies
                .iter()
                .filter_map(|t| t.fields.iter().find(|f| f.key == fw.key))
                .collect();
            if fw.readonly && !fs.iter().all(|f| f.readonly) {
                out.push(format!("{d}.{} readonly", fw.key));
            }
            let mu = fs.iter().fold(0u16, |m, f| m | f.kind.mask());
            if mu & !fw.kind.mask() != 0 {
                out.push(format!(
                    "{d}.{} kind {mu:#x} vs {:#x}",
                    fw.key,
                    fw.kind.mask()
                ));
            }
        }
    }
    assert_eq!(seen, 1, "one literal with fields {keys:?}");
    out
}

/// The per-unit run claims about the literal with fields `keys` all that
/// the whole-program run does.
fn assert_literal_as_precise(src: &str, keys: &[&str]) {
    let u = units(src);
    let w = whole(src);
    let s = literal_weaker(&u, &w, keys);
    assert!(
        s.is_empty(),
        "per-unit weaker: {s:?}\n{}\n---\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

fn assert_never_stronger(src: &str) {
    let u = units(src);
    let w = whole(src);
    let s = templates_stronger(&u, &w);
    assert!(
        s.is_empty(),
        "per-unit stronger: {s:?}\n{}\n---\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

#[test]
fn another_units_store_into_object_prototype_reaches_this_units_objects() {
    // `patch` (a unit of its own) replaces `Object.prototype.hasOwnProperty`
    // with a function that writes a string into its receiver's `n`; the
    // module scope captures it and calls it on its own literal. Without the
    // replay the literal's unit sees the builtin method (a pure call), and
    // claims `n` a readonly number: `twice()` read the string as a number.
    assert_never_stronger(
        r#"
        function patch(): void {
          (Object.prototype as any).hasOwnProperty = function (this: any, k: string): boolean {
            this.n = "s" + k;
            return true;
          };
        }
        patch();
        var hop = Object.prototype.hasOwnProperty;
        var cache = { n: 1, m: 2 };
        function check(k: string): boolean { return hop.call(cache, k); }
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log(check("z"), twice());
        "#,
    );
}

#[test]
fn another_units_accessor_on_object_prototype_reaches_this_units_objects() {
    // An accessor on `Object.prototype` runs for every lookup that reaches
    // it (E23, whole-program facts claim nothing); installed by another
    // unit, it is that for this unit too.
    assert_never_stronger(
        r#"
        function install(): void {
          Object.defineProperty(Object.prototype, "x", {
            get(this: any) { this.n = "s"; return 1; },
            configurable: true,
          });
        }
        install();
        var cache = { n: 1, m: 2 };
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log((cache as any).x, twice());
        "#,
    );
}

#[test]
fn a_shared_object_handed_to_another_unit_takes_its_stores_into_top() {
    // `fill` (a unit of its own) stores into its parameter, ⊤ there; the
    // module scope hands it `Object.prototype`. The store is a store into
    // `Object.prototype` (whole-program facts see it on its token).
    assert_never_stronger(
        r#"
        function fill(t: any, k: string, v: any): void { t[k] = v; }
        fill(Object.prototype, "hasOwnProperty", function (this: any, k: string): boolean {
          this.n = "s" + k;
          return true;
        });
        var hop = Object.prototype.hasOwnProperty;
        var cache = { n: 1, m: 2 };
        function check(k: string): boolean { return hop.call(cache, k); }
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log(check("z"), twice());
        "#,
    );
}

#[test]
fn explain_counts_every_instance_kind() {
    // A for-in body's run is instance kind 6 (`Summaries::inst_key`): the
    // explain statistics count it instead of indexing past their table.
    let src = r#"
        const schema = { properties: { a: 1, b: 2 } };
        function count(data: any): number {
          let n = 0;
          for (const k in data) {
            if (k in schema.properties) n++;
          }
          return n;
        }
        console.log(count({ a: 1, z: 2 }), count({ b: 1 }));
        "#;
    let h = hir(src);
    let cfg = FactsConfig {
        explain: true,
        ..Default::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &h)], &cfg);
    assert!(!pf.templates.is_empty(), "{}", pf.dump_text());
}

#[test]
fn a_binding_holding_a_shared_object_names_it_to_its_readers() {
    // `P` holds `Object.prototype`; `put` (a unit of its own) reads it
    // through the binding's value contract and stores into it. The contract
    // names the runtime-owned object (not nothing, as it did): `put` holds
    // it, and its store is a store into `Object.prototype`.
    assert_never_stronger(
        r#"
        var P: any = Object.prototype;
        function put(): void {
          P.hasOwnProperty = function (this: any, k: string): boolean {
            this.n = "s" + k;
            return true;
          };
        }
        put();
        var hop = Object.prototype.hasOwnProperty;
        var cache = { n: 1, m: 2 };
        function check(k: string): boolean { return hop.call(cache, k); }
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log(check("z"), twice());
        "#,
    );
}

#[test]
fn a_shared_object_stored_into_another_units_literal_is_handed_over() {
    // `setp` stores `Object.prototype` into the module scope's literal (a
    // store effect, whose contract names it: the literal's unit holds it);
    // `put` reads it back through the literal's layout, whose fields hold
    // unknown values for the reader: the literal's unit hands it over where
    // its fields leave, and `put`'s store into ⊤ is a store into
    // `Object.prototype`.
    assert_never_stronger(
        r#"
        var holder: any = { p: null };
        function setp(): void { holder.p = Object.prototype; }
        function put(): void {
          holder.p.hasOwnProperty = function (this: any, k: string): boolean {
            this.n = "s" + k;
            return true;
          };
        }
        setp();
        put();
        var hop = Object.prototype.hasOwnProperty;
        var cache = { n: 1, m: 2 };
        function check(k: string): boolean { return hop.call(cache, k); }
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log(check("z"), twice());
        "#,
    );
}

#[test]
fn a_shared_object_given_to_unknown_code_takes_its_stores_into_top() {
    // `fire` (a unit of its own) calls what it reads out of `hooks` (⊤
    // there: the array's elements hold unknown values for the reader) with
    // `Object.prototype`. Unknown code includes the other units' code: the
    // function the module scope registered stores into its parameter, and
    // that store is a store into `Object.prototype`.
    assert_never_stronger(
        r#"
        var hooks: any[] = [];
        function register(f: any): void { hooks.push(f); }
        register(function (p: any): void {
          p.hasOwnProperty = function (this: any, k: string): boolean {
            this.n = "s" + k;
            return true;
          };
        });
        function fire(): void {
          for (var i = 0; i < hooks.length; i++) hooks[i](Object.prototype);
        }
        fire();
        var hop = Object.prototype.hasOwnProperty;
        var cache = { n: 1, m: 2 };
        function check(k: string): boolean { return hop.call(cache, k); }
        function twice(): number { return cache.n * 2 + cache.m; }
        console.log(check("z"), twice());
        "#,
    );
}

#[test]
fn a_binding_naming_object_prototype_hands_nothing_over() {
    // `P` holds `Object.prototype` and its readers hold it by name: nothing
    // is given to unknown code. `def` installs an accessor on its parameter
    // (⊤ there), which is the module scope's own literal. Had `P` handed
    // `Object.prototype` over, that accessor would be one on it (E23) for
    // every unit, and `lit`'s conversion to a string would run it with
    // `lit` as its receiver.
    assert_literal_as_precise(
        r#"
        var P: any = Object.prototype;
        function show(): string { return typeof P.toString; }
        function def(o: any): void {
          Object.defineProperty(o, "zz", { get: function () { return 1; }, configurable: true });
        }
        var box = {};
        def(box);
        var lit = { a: 1, b: 2 };
        function str(): string { return "" + lit; }
        console.log(show(), str(), (box as any).zz);
        "#,
        &["a", "b"],
    );
}

#[test]
fn a_prototype_given_to_object_create_hands_nothing_over() {
    // `O.create` (a call through a value holding the `Object` namespace)
    // makes `Object.prototype` the prototype of a new object: reachable only
    // along that object's chain, it is no runtime-owned object given to
    // unknown code, and `def`'s accessor on its parameter is no accessor on
    // `Object.prototype`.
    assert_literal_as_precise(
        r#"
        var O: any = Object;
        var made = O.create(Object.prototype);
        function def(o: any): void {
          Object.defineProperty(o, "zz", { get: function () { return 1; }, configurable: true });
        }
        var box = {};
        def(box);
        var lit = { a: 1, b: 2 };
        function str(): string { return "" + lit; }
        console.log(typeof made, str(), (box as any).zz);
        "#,
        &["a", "b"],
    );
}

#[test]
fn a_namespace_method_keeps_its_namespace() {
    // An unmodeled `Object` method called through a value runs on its own
    // namespace: the runtime gives `Object` to no one, so nothing is handed
    // over and no unit's store into ⊤ is a store into `Object`.
    use crate::model::{BuiltinNs, Shared, SharedWrite};
    let pf = units(
        r#"
        var O: any = Object;
        var d = O.getOwnPropertyDescriptors({ q: 1 });
        console.log(typeof d);
        "#,
    );
    let handed = SharedWrite::Handed(Shared::Builtin(BuiltinNs::Object));
    assert!(!pf.shared_out.contains(&handed), "{:?}", pf.shared_out);
}

#[test]
fn another_units_accessor_on_a_namespace_is_one_key() {
    // `install` puts an accessor on the `Object` namespace under `zz`; the
    // module scope reads it there. Its replay is an accessor under that key
    // (another unit's function), not a store under every key of `Object`.
    use crate::model::{BuiltinNs, Shared, SharedWrite};
    let pf = units(
        r#"
        function install(): void {
          Object.defineProperty(Object, "zz", { get: function () { return 1; }, configurable: true });
        }
        install();
        console.log((Object as any).zz);
        "#,
    );
    let o = Shared::Builtin(BuiltinNs::Object);
    assert!(
        pf.shared_out
            .contains(&SharedWrite::Accessor(o.clone(), "zz".into())),
        "{:?}",
        pf.shared_out
    );
    assert!(
        !pf.shared_out.contains(&SharedWrite::Store(o, None)),
        "{:?}",
        pf.shared_out
    );
}
