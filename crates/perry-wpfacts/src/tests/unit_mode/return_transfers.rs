use super::*;

#[test]
fn a_pure_parameter_return_keeps_each_callers_function() {
    let s = "export const lambda = (f: any): any => f;";
    let m = "import * as S from './s'; function fa() { return 11; } function fb() { return 29; } const a = S.lambda(fa); const b = S.lambda(fb); console.log(a(), b());";
    let mods = [("s", s), ("m", m)];
    for (mode, pf) in [("whole", whole_of(&mods)), ("unit", units_of(&mods))] {
        let mut calls: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.key == "()")
            .collect();
        calls.sort_by_key(|s| s.byte_offset);
        assert_eq!(calls.len(), 2, "{mode}\n{}", pf.dump_text());
        for (call, name) in calls.iter().zip([":fa", ":fb"]) {
            assert_eq!(
                call.targets.len(),
                1,
                "{mode}: {call:?}\n{}",
                pf.dump_text()
            );
            assert!(
                call.targets[0].split('#').next().unwrap().ends_with(name),
                "{mode}: {call:?}"
            );
        }
    }
}

#[test]
fn a_result_with_another_source_is_not_a_pure_parameter_return() {
    let s = "export const pick = (f: any, flag: boolean): any => flag ? f : Math.abs;";
    let m = "import * as S from './s'; function fa() { return 11; } const f = S.pick(fa, !!(globalThis as any).flag); console.log(f(-29));";
    let mods = [("s", s), ("m", m)];
    for pf in [whole_of(&mods), units_of(&mods)] {
        let calls: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.key == "()")
            .collect();
        assert_eq!(calls.len(), 1, "{}", pf.dump_text());
        assert!(
            matches!(calls[0].verdict, Verdict::No(_)),
            "{}",
            pf.dump_text()
        );
    }
}
