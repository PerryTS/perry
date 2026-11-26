//! A site is (module, start, kind): a compound's GetValue and PutValue
//! share their member token, and an ordinary read of the same key on the
//! base stands at its own token; each keeps its own verdict.
use super::*;
use perry_codegen::AccessorKind;

// `o.k.k += 1`: the inner `o.k` (an ordinary read, A's getter) stands at
// the first `k`; the compound's GetValue and PutValue of `.k` on its value
// (B's data field) stand at the second `k` (`perry_hir::lower::member_token`).
const SOURCE: &str = "class B { k = 1; } \
     class A { b = new B(); get k() { return this.b; } } \
     const o = new A(); \
     o.k.k += 1; \
     console.log(o.b.k);";

#[test]
fn ordinary_read_and_compound_get_keep_their_verdicts() {
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
    assert!(
        read_at < update_at,
        "each read stands at its own member token"
    );
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
    let entry = |at, kind| out.accessors.get(&(at, "k".to_string(), kind));
    // The ordinary read calls A's getter; the compound's GetValue reads B's
    // field and its PutValue stores it: neither is an accessor site.
    assert!(
        entry(read_at, AccessorKind::Get).is_some(),
        "{:?}",
        out.accessors
    );
    for at in [read_at, update_at] {
        assert!(entry(at, AccessorKind::UpdateGet).is_none());
        assert!(entry(at, AccessorKind::Set).is_none());
    }
    assert!(entry(update_at, AccessorKind::Get).is_none());
}
