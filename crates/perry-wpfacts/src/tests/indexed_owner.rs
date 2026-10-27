use crate::{compute, compute_units, FactsConfig, ProgramFacts};

fn run(src: &str, units: bool) -> (perry_hir::Module, ProgramFacts) {
    let ast = perry_parser::parse_typescript(src, "owner.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "owner", "owner.ts").unwrap();
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..Default::default()
    };
    let input = [("owner.ts".to_string(), &hir)];
    let facts = if units {
        compute_units(&input, &cfg)
    } else {
        compute(&input, &cfg)
    };
    assert!(!facts.stats.aborted);
    (hir, facts)
}
fn binding(hir: &perry_hir::Module, name: &str) -> u32 {
    let mut found = None;
    perry_hir::tdz_check::for_each_stmt_deep(&hir.init, &mut |s| {
        if let perry_hir::Stmt::Let { id, name: n, .. } = s {
            if n == name {
                found = Some(*id);
            }
        }
    });
    found.unwrap()
}
fn owner<'a>(
    hir: &perry_hir::Module,
    facts: &'a ProgramFacts,
    name: &str,
) -> Option<&'a crate::bindings::IndexedOwner> {
    let id = binding(hir, name);
    facts
        .bindings_for("owner.ts")?
        .locals
        .iter()
        .find(|(i, _)| *i == id)?
        .1
        .indexed_owner
        .as_ref()
}
#[test]
fn fresh_owner_return_and_closed_argument_keep_the_allocation_contract() {
    let src = r#"
        const N = 8;
        function make() { const a = new Float64Array(N * N); a[0] = 3; return a; }
        const x = make();
        function use(a: Float64Array) { return a[0] + a[1]; }
        console.log(use(x));
    "#;
    for units in [false, true] {
        let (h, f) = run(src, units);
        let p = owner(&h, &f, "x").expect("fresh returned owner reaches caller");
        assert_eq!((p.kind, p.min_len, p.exact_len), (7, 64, Some(64)));
        assert!(!p.allocations.is_empty());
    }
}
#[test]
fn views_exposure_mixed_values_and_bad_annotations_cannot_publish_a_fresh_owner() {
    for expr in [
        "new Float64Array(new ArrayBuffer(64))",
        "({0: 3, length: 8} as unknown as Float64Array)",
        "(Math.random() ? new Float64Array(8) : new Int32Array(8))",
    ] {
        let src = format!("const x = {expr}; console.log(x[0]);");
        for units in [false, true] {
            let (h, f) = run(&src, units);
            assert!(owner(&h, &f, "x").is_none(), "{expr}, units={units}");
        }
    }
    let src = "const x = new Float64Array(8); function expose(a: Float64Array) { return a.buffer; } console.log(expose(x).byteLength);";
    for units in [false, true] {
        let (h, f) = run(src, units);
        assert!(
            owner(&h, &f, "x").is_none(),
            "cross-unit exposure revokes the owner"
        );
    }
}
#[test]
fn mutable_extents_are_not_published_as_constant_allocation_lengths() {
    for units in [false, true] {
        let (h, f) = run(
            "let n = 8; n = 16; const x = new Float64Array(n); console.log(x[0]);",
            units,
        );
        assert!(owner(&h, &f, "x").is_none_or(|p| p.min_len == 0));
    }
}

#[test]
fn allocation_identity_includes_the_declaration_unit() {
    let src = r#"
        const x = new Int32Array(256);
        const bytes = new Uint8Array(8192);
        function read(data: Uint8Array) { return x[data[0] & 255] + data.length; }
        console.log(read(bytes));
    "#;
    let (h, f) = run(src, true);
    let x = owner(&h, &f, "x").unwrap();
    let bytes = owner(&h, &f, "bytes").unwrap();
    assert_eq!((x.kind, x.exact_len), (4, Some(256)));
    assert_eq!((bytes.kind, bytes.exact_len), (1, Some(8192)));
    assert!(x.allocations.iter().all(|a| !bytes.allocations.contains(a)));
    let function = h.functions.iter().find(|f| f.name == "read").unwrap();
    let param = function.params[0].id;
    let facts = f
        .bindings_for("owner.ts")
        .unwrap()
        .locals
        .iter()
        .find(|(id, _)| *id == param)
        .unwrap();
    assert_eq!(facts.1.indexed_owner.as_ref().unwrap().kind, 1);
}
#[test]
fn prototype_writes_withhold_the_global_metadata_effect_proof() {
    for units in [false, true] {
        let (_, f) = run("Object.defineProperty(Uint8Array.prototype, 'length', {get() { return 7; }}); const x = new Uint8Array(4); console.log(x.length);", units);
        assert!(!f.indexed_metadata_pristine());
    }
}

#[test]
fn numeric_constructor_arguments_keep_owner_identity_without_inventing_an_extent() {
    for units in [false, true] {
        let (h, f) = run("function make(n: number) { return new Float64Array(n); } const x = make(8); console.log(x.length);", units);
        let o = owner(&h, &f, "x").expect("numeric argument contract proves fresh construction");
        assert_eq!((o.kind, o.min_len, o.exact_len), (7, 0, None));
        let (h, f) = run("function make(n: number) { return new Float64Array(n); } const x = make(new ArrayBuffer(64) as unknown as number); console.log(x.length);", units);
        assert!(
            owner(&h, &f, "x").is_none(),
            "a source/view overload is never a numeric fresh-owner contract"
        );
    }
}

#[test]
fn a_unit_over_its_work_budget_cannot_certify_program_wide_metadata_effects() {
    let ast = perry_parser::parse_typescript("const x = new Uint8Array(4); Object.defineProperty(Uint8Array.prototype, length, {get() {return 2;}}); console.log(x.length);", "budget.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "budget", "budget.ts").unwrap();
    let facts = compute_units(
        &[("budget.ts".into(), &hir)],
        &FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    );
    assert!(facts.units.iter().any(|(_, s)| s.aborted));
    assert!(!facts.indexed_metadata_pristine());
}
