//! Numeric-element operations must not read intrinsic methods as elements.
use super::*;

#[test]
fn sorting_and_copying_records_pass_only_the_elements_to_callbacks() {
    for operation in [
        "xs.sort(compare); xs.map(read);",
        "xs.slice().sort(compare); xs.slice().map(read);",
        "xs.toSorted(compare).map(read);",
        "xs.toReversed().map(read);",
        "xs.toSpliced(0, 0).map(read);",
        "xs.with(0, new Item()).map(read);",
        "read(xs.at(0));",
        "read(xs.pop());",
        "read(xs.shift());",
        "xs.splice(0, 1).map(read);",
        "xs.reduce((acc: any, item: any) => acc + read(item), 0);",
    ] {
        let pf = facts(&format!(
            "class Item {{ rank = 1; }}
             function compare(a: any, b: any) {{ return a.rank - b.rank; }}
             function read(item: any) {{ return item.rank; }}
             const xs = [new Item(), new Item()]; {operation}"
        ));
        let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "rank").collect();
        let live: Vec<_> = sites.iter().filter(|s| !s.receivers.is_empty()).collect();
        assert!(
            !live.is_empty(),
            "{operation}: no live reads\n{}",
            pf.dump_text()
        );
        for s in live {
            assert_eq!(
                s.verdict,
                Verdict::T0Mono,
                "{operation}\n{}",
                pf.dump_text()
            );
            assert_eq!(s.receivers, ["t.ts:Item"], "{operation}");
        }
    }
}

#[test]
fn an_actual_builtin_function_element_is_still_a_callback_argument() {
    let pf = facts(
        "class Item { rank = 1; }
         function compare(a: any, b: any) { return a.rank - b.rank; }
         const xs = [new Item(), Math.abs]; xs.sort(compare);",
    );
    let reads: Vec<_> = pf.sites.iter().filter(|s| s.key == "rank").collect();
    assert!(!reads.is_empty(), "{}", pf.dump_text());
    assert!(
        reads.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_copy_keeps_an_actual_escape_of_its_record_elements() {
    let pf = facts(
        "class Item { rank = 1; }
         declare function keep(x: any): void;
         function read(item: any) { return item.rank; }
         const xs = [new Item()]; keep(xs); xs.slice().map(read);",
    );
    let reads: Vec<_> = pf.sites.iter().filter(|s| s.key == "rank").collect();
    assert!(!reads.is_empty(), "{}", pf.dump_text());
    assert!(
        reads.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        pf.dump_text()
    );
}
