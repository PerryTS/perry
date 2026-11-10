//! The chain of a container the model fills (`Family::Box`) is the one its
//! creator gives it (`members::box_chain`): a `JSON.parse` object is an
//! ordinary object, so past its own values (never callable) a read finds
//! `Object.prototype` only, where an absent key is final (DEC 250) even
//! once a store into ⊤ under a computed key may have written every other
//! builtin prototype; an `Object.groupBy` result has no chain at all. A
//! `Map.groupBy` Map, an undescribed method's result (`primbox`), a given
//! prototype and the exempt `constructor` keep the opaque answer.
//! (three.core.js `PropertyBinding.sanitizeNodeName`: `name.replace(..)` on
//! a GLTF JSON field.)

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// `a` is held by the receiver `body` makes; the receiver escapes only if
/// the call in `body` is an unknown method. A store into ⊤ under a computed
/// key runs first, so every key may be written on the add-open prototypes.
fn held(body: &str) -> String {
    format!(
        "class A {{ a = 1; }}
         const unknown: any = (globalThis as any)[process.argv[2]];
         const t: any = unknown.reach(0);
         const kk: string = process.argv[3];
         t[kk] = 1;
         const a = new A();
         {body}
         console.log(a.a);"
    )
}

/// An ordinary object a builtin makes (an `Object.fromEntries` result):
/// an absent method is final. (A `JSON.parse` value may also be an array,
/// whose `Array.prototype` takes named adds: its method may be written,
/// rightly; a property descriptor's own values hold ⊤, its accessors.)
#[test]
fn an_absent_method_of_an_ordinary_builtin_object_is_final_after_a_computed_top_store() {
    for make in ["Object.fromEntries([[process.argv[4] ?? 'q', 1]])"] {
        let src = held(&format!(
            "const j: any = {make};
             j.held = a;
             try {{ console.log(j.replace(/a/g, 'b')); }} catch (e) {{ console.log('TypeError'); }}"
        ));
        let pf = facts(&src);
        assert!(proven(&pf, "a"), "{src}\n{}", pf.dump_text());
    }
}

/// A primitive receiver's written method runs on the primitive: the
/// objects beside it in the receiver node are not handed to it.
#[test]
fn a_written_primitive_method_does_not_take_the_objects_of_the_receiver_node() {
    let src = held(
        "const j: any = process.argv.length > 9 ? 7 : Object.fromEntries([['q', 1]]);
         j.held = a;
         try { console.log(j.replace('x', 'y')); } catch (e) { console.log('TypeError'); }",
    );
    let pf = facts(&src);
    assert!(proven(&pf, "a"), "{src}\n{}", pf.dump_text());
}

#[test]
fn an_object_group_by_result_has_no_chain_a_map_group_by_result_keeps_its_own() {
    for (make, closed) in [("Object.groupBy", true), ("Map.groupBy", false)] {
        let src = held(&format!(
            "const g: any = {make}([1, 2], (x: number) => (x > 1 ? 'k' : 'j'));
             g.held = a;
             try {{ console.log(g.replace('x')); }} catch (e) {{ console.log('TypeError'); }}"
        ));
        let pf = facts(&src);
        assert_eq!(proven(&pf, "a"), closed, "{src}\n{}", pf.dump_text());
    }
}

/// Controls: what may still be found. `constructor` is the one key the
/// lock exempts; a prototype given to the object is read before the
/// intrinsic end.
#[test]
fn a_json_object_keeps_constructor_and_a_given_prototype() {
    for body in [
        "const j: any = JSON.parse(process.argv[4] ?? '{}');
         j.held = a;
         try { console.log(j.constructor('x')); } catch (e) { console.log('TypeError'); }",
        "const j: any = JSON.parse(process.argv[4] ?? '{}');
         Object.setPrototypeOf(j, { replace(x: any) { unknown.take(x); return 1; } });
         try { console.log(j.replace(a)); } catch (e) { console.log('TypeError'); }",
    ] {
        let src = held(body);
        let pf = facts(&src);
        assert!(!proven(&pf, "a"), "{src}\n{}", pf.dump_text());
    }
}

/// `JSON.parse` called as a namespace member makes arrays as well as
/// objects: an array's `forEach` runs its callback.
#[test]
fn a_json_parse_through_the_namespace_makes_arrays_too() {
    let src = "class A { a = 1; }
        const unknown: any = (globalThis as any)[process.argv[2]];
        const a = new A();
        const J: any = JSON;
        const arr: any = J.parse(process.argv[4] ?? '[1]');
        try { arr.forEach((x: any) => unknown.take(a)); } catch (e) { console.log('TypeError'); }
        console.log(a.a);";
    let pf = facts(src);
    assert!(!proven(&pf, "a"), "{src}\n{}", pf.dump_text());
}
