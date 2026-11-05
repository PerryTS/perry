//! Number-key writes retain their key range on every property holder.
//! The solver's NUM value fact is the producer; annotations and numeric-looking
//! strings do not narrow an arbitrary key. Numeric reads retain the value.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let ast = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn check(src: &str, want_proven: bool) {
    let pf = one(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "x" && !s.is_call).collect();
    assert!(!sites.is_empty(), "no x read");
    assert!(sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_)) == want_proven),
        "want proven={want_proven}: {}", pf.dump_text());
}

fn program(key: &str, value: &str, member: &str) -> String {
    format!("function mutate(z: any) {{ delete z.x; }}
        function main() {{
          (Object as any)[{key}] = {value};
          const q: any = {{x: 1}};
          const f: any = (Object as any)[{member}];
          if (typeof f === 'function') f(q);
          return q.x;
        }} console.log(main());")
}

#[test]
fn numeric_store_cannot_supply_a_nonnumeric_named_member() {
    check(&program("+process.argv[2]", "1", "'zz'"), true);
    check(&program("+process.argv[2]", "mutate", "'zz'"), true);
}

#[test]
fn every_canonical_number_name_and_numeric_read_retains_the_store() {
    for name in ["0", "NaN", "Infinity", "-Infinity", "1e+21", "1.5", "-1"] {
        check(&program("+process.argv[2]", "mutate", &format!("'{name}'")), false);
    }
    check(&program("-0", "mutate", "'0'"), false);
    check(&program("+process.argv[2]", "mutate", "+process.argv[3]"), false);
}

#[test]
fn strings_mixed_values_symbols_and_annotations_do_not_prove_a_number_key() {
    for key in ["process.argv[2]", "String(process.argv[2])",
        "(process.argv.length > 2 ? 'z' : 0) + 1",
        "(process.argv.length > 2 ? Symbol() : +process.argv[2])"] {
        check(&program(key, "mutate", "'z1'"), false);
    }
    check("function mutate(z: any) { delete z.x; }
        const j: number = process.argv[2] as any;
        (Object as any)[j + 1] = mutate;
        const q: any = {x:1}; const f: any = (Object as any).z1;
        if (typeof f === 'function') f(q); console.log(q.x);", false);
}

#[test]
fn object_keys_still_run_to_property_key_and_stay_arbitrary() {
    check("const q: any = {x:1};
        const k: any = {toString() { delete q.x; return '0'; }};
        (Object as any)[k] = 1; console.log(q.x);", false);
    check(&program("({toString() { return 'zz'; }})", "mutate", "'zz'"), false);
}

#[test]
fn numeric_range_flows_through_spread_without_becoming_every_name() {
    for (member, proven) in [("'zz'", true), ("'0'", false)] {
        check(&format!("function mutate(z: any) {{ delete z.x; }}
            const a: any = {{}}; a[+process.argv[2]] = mutate;
            const b: any = {{...a}};
            const q: any = {{x:1}}; const f: any = b[{member}];
            if (typeof f === 'function') f(q); console.log(q.x);"), proven);
    }
}

#[test]
fn exact_numeric_keys_remain_distinct_and_array_values_remain_visible() {
    check("function mutate(z: any) { delete z.x; }
        const a: any = {}; a['0'] = mutate;
        const q: any = {x:1}; const f: any = a['1'];
        if (typeof f === 'function') f(q); console.log(q.x);", true);
    check("function mutate(z: any) { delete z.x; }
        const a: any = []; a[+process.argv[2]] = mutate;
        const q: any = {x:1}; const f: any = a[+process.argv[3]];
        if (typeof f === 'function') f(q); console.log(q.x);", false);
}

#[test]
fn descriptor_stores_and_arbitrary_constructor_writes_are_unchanged() {
    check("function mutate(z: any) { delete z.x; }
        Object.defineProperty(Object, 'zz', {value:mutate, writable:true, configurable:true});
        const q: any = {x:1}; const f: any = (Object as any).zz;
        if (typeof f === 'function') f(q); console.log(q.x);", false);
    check(&program("process.argv[2]", "mutate", "'constructor'"), false);
}
