//! A site is (module, start, kind): an ordinary read and a compound's
//! GetValue standing at one offset with one key keep their own verdicts.
use super::*;
use perry_codegen::AccessorKind;

// `o.k.k += 1`: the inner `o.k` (an ordinary read, A's getter) and the
// compound's GetValue of `.k` on its value (B's data field) both stand at
// the member's start, `o`, with key `k`.
const SOURCE: &str = "class B { k = 1; } \
     class A { b = new B(); get k() { return this.b; } } \
     const o = new A(); \
     o.k.k += 1; \
     console.log(o.b.k);";

#[test]
fn ordinary_read_and_compound_get_at_one_offset_keep_their_verdicts() {
    let ast = perry_parser::parse_typescript(SOURCE, "main.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "main", "main.ts").unwrap();
    let pf = perry_wpfacts::compute(
        &[("main.ts".into(), &hir)],
        &perry_wpfacts::FactsConfig {
            namespace_pairs: namespace_pairs_on(),
            ..Default::default()
        },
    );
    let at = |access: perry_wpfacts::derive::Access| {
        let s = pf
            .sites
            .iter()
            .find(|s| !s.is_call && s.key == "k" && s.access == access)
            .unwrap_or_else(|| panic!("no {access:?} site of k"));
        (s.byte_offset, s.verdict.clone())
    };
    let (read_at, read_verdict) = at(perry_wpfacts::derive::Access::Read);
    let (update_at, update_verdict) = at(perry_wpfacts::derive::Access::UpdateGet);
    assert_eq!(read_at, update_at, "the two reads share the member's start");
    assert!(matches!(read_verdict, perry_wpfacts::Verdict::AccessorGet));
    assert!(!matches!(
        update_verdict,
        perry_wpfacts::Verdict::AccessorGet
    ));
    let out = elision_for(&pf, "main.ts", &|path, class, function, key, member| {
        assert_eq!(path, "main.ts");
        accessor_symbol_in(&hir, "main", class, function, key, member == Member::Set)
            .map(|s| (s, perry_codegen::DirectAbi::default()))
    });
    let entry = |kind| out.accessors.get(&(read_at, "k".to_string(), kind));
    // The ordinary read calls A's getter; the compound's GetValue reads B's
    // field and its PutValue stores it: neither is an accessor site.
    assert!(entry(AccessorKind::Get).is_some(), "{:?}", out.accessors);
    assert!(entry(AccessorKind::UpdateGet).is_none());
    assert!(entry(AccessorKind::Set).is_none());
}
