//! The rest element of an array destructuring (`[a, ...rest] = v`) is the
//! array the runtime fills with the remaining iterated values
//! (`iteratorRestAppend`, what codegen lowers to `js_array_push_f64`): its
//! elements are those values, and nothing of them reaches unmodeled code.

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

#[test]
fn a_rest_element_holds_the_remaining_elements() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        class Bag {
          items: B[];
          constructor() { this.items = [new B(), new B()]; }
          [Symbol.iterator]() {
            let i = 0;
            const items = this.items;
            return { next: () => i < items.length ? { value: items[i++], done: false } : { value: undefined, done: true } };
          }
        }
        // One destructuring per function: a function's iterations share
        // one output node (`iter_out_node`).
        function arr() {
          const xs: any[] = [new A(), new A(), new A()];
          const [first, ...rest] = xs;
          console.log(first.a, rest[0].a, rest.length);
        }
        function bag() {
          const [...all] = new Bag();
          console.log(all[0].b);
        }
        arr(); bag();
        "#,
    );
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    // The hand-written iterable's own protocol is imprecise beside it (its
    // `this` holds its iterator too): the rest array holds what it yields,
    // with no ⊤.
    let b = site(&pf, "b");
    assert!(
        !b.top && b.receivers.iter().any(|r| r == "t.ts:B"),
        "{}",
        pf.dump_text()
    );
    assert!(tmpl(&pf, "A").escape.is_none(), "{}", pf.dump_text());
    assert!(tmpl(&pf, "B").escape.is_none(), "{}", pf.dump_text());
}

/// Soundness: the rest array is an ordinary array of the program: what is
/// stored into it later is read back beside the iterated values.
#[test]
fn a_rest_element_is_an_ordinary_array() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class C { a = "c"; }
        const xs: any[] = [new A(), new A()];
        const [first, ...rest] = xs;
        rest.push(new C());
        for (const r of rest) console.log(r.a);
        console.log(first.a);
        "#,
    );
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "a" && s.receivers.len() == 2)
        .collect();
    assert!(
        s.iter().any(|s| {
            !s.top
                && s.receivers.iter().any(|r| r.ends_with(":A"))
                && s.receivers.iter().any(|r| r.ends_with(":C"))
        }),
        "{}",
        pf.dump_text()
    );
}
