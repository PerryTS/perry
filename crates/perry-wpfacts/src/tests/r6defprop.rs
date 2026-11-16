//! R6DEFPROP1 (three.js r6: 109 of 171 not-proven records): a constructor's
//! `Object.defineProperty(this, "id", { value: n++ })` (Source, Texture,
//! Object3D, BufferAttribute, BufferGeometry, Material) and Object3D's
//! `Object.defineProperties(this, { position: { configurable: true,
//! enumerable: true, value: p }, ... })` mark the class instance E5 HARD, and
//! every read of ANY key of it (and of every subclass: Bone, Mesh, ...) is
//! refused with `hard: E5 E5 Object.defineProperty`.
//! ECMA-262 20.1.2.4 -> 7.3.8 DefinePropertyOrThrow -> 10.1.6.3
//! ValidateAndApplyPropertyDescriptor: a data descriptor under the named key
//! P, P absent, creates P alone (step 2.c: [[Value]] and the attributes the
//! descriptor gives, others false); no other own key of the object is
//! created, changed or removed, and no accessor is installed. The reads of the
//! other keys keep their facts. The read of P itself is not a plain birth slot.
use super::*;

fn witness(src: &str, key: &str) -> Verdict {
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .collect();
    assert!(!sites.is_empty(), "missing {key} site");
    assert_eq!(sites.len(), 1, "{sites:?}");
    sites[0].verdict.clone()
}

fn prog(def: &str) -> String {
    format!(
        "let n = 0;
         class Obj3 {{
           uuid: string; witness: number;
           constructor() {{ {def}; this.uuid = 'u' + n; this.witness = 1; }}
         }}
         class Bone extends Obj3 {{ isBone: boolean; constructor() {{ super(); this.isBone = true; }} }}
         const b = new Bone();
         const w: number = b.witness;
         console.log(w + 1, (b as any).id);"
    )
}

const ID: &str = "Object.defineProperty(this, 'id', { value: n++ })";
const PROPS: &str = "const p = { x: 0 }; Object.defineProperties(this, { position: { configurable: true, enumerable: true, value: p } })";

/// RED today: `hard: E5 E5 Object.defineProperty`.
#[test]
fn r6defprop_named_data_define_keeps_other_key() {
    let v = witness(&prog(ID), "witness");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// RED today: `hard: E5 E5 Object.defineProperties`.
#[test]
fn r6defprop_named_data_define_properties_keeps_other_key() {
    let v = witness(&prog(PROPS), "witness");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// Keep: the defined key is no plain birth slot (non-writable,
/// non-enumerable, non-configurable data).
#[test]
fn r6defprop_keep_defined_key_not_plain() {
    let v = witness(&prog(ID), "id");
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}

/// Keep: a computed key may name any key of the object (any of its slots).
#[test]
fn r6defprop_keep_computed_key_hard() {
    let v = witness(
        &prog("Object.defineProperty(this, process.argv[2], { value: 1 })"),
        "witness",
    );
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}

/// Keep: redefining a birth key changes the attributes of its slot.
#[test]
fn r6defprop_keep_birth_key_redefine_hard() {
    let v = witness(
        &prog("this.witness = 0; Object.defineProperty(this, 'witness', { value: 2 })"),
        "witness",
    );
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}

fn all_no(src: &str, key: &str) -> usize {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!sites.is_empty(), "missing {key} site");
    for s in &sites {
        assert!(matches!(s.verdict, Verdict::No(_)), "{key}: {s:?}");
    }
    sites.len()
}

/// (a) Enumeration: `{ value: v }` defines a NON-enumerable key (6.2.6.5,
/// absent [[Enumerable]] = false), so a spread copy (CopyDataProperties,
/// 7.3.25: enumerable own keys only) does not hold it, and the source has no
/// exact key list after the define (`exact_keys`: `adds_keys`): the copy's
/// read of `id` is own data or absent (the copy is a plain literal; absent
/// is what the spec gives), never a slot of a fixed layout born with `id`.
#[test]
fn r6defprop_enumeration_does_not_list_defined_key() {
    let src = "const o: any = { a: 1 };
         Object.defineProperty(o, 'id', { value: 7 });
         const c: any = { ...o };
         console.log(c.id);";
    let pf = facts(src);
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "id" && !s.is_call)
        .collect();
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(
        !matches!(s[0].verdict, Verdict::T0Mono | Verdict::T0Set),
        "{:?}",
        s[0]
    );
    // for-in over the class instance: the defined key is no exact key.
    let n = all_no(
        &prog(&format!(
            "{ID}; for (const k in this) {{ (this as any)['x' + k] = 1; }}"
        )),
        "id",
    );
    assert_eq!(n, 1);
}

/// (b) A store to the defined (non-writable) key throws in strict code
/// (10.1.9.2 OrdinarySetWithOwnDescriptor step 2.a -> false, 7.3.4 Set
/// throws), and a delete of it (non-configurable) is refused: neither is a
/// plain slot write (a store is no read site: the read after it stays
/// unproven); the delete keeps the object HARD (E7) as before.
#[test]
fn r6defprop_store_and_delete_of_defined_key_stay_unproven() {
    let src = prog(ID).replace("console.log(", "(b as any).id = 5; console.log(");
    assert_eq!(all_no(&src, "id"), 1);
    let src = prog(ID).replace("console.log(", "delete (b as any).id; console.log(");
    let v = witness(&src, "witness");
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}

/// (c) The defined key is not an accessor key of the token (that refuses
/// every key of it, derive `own_data_or_absent_why`): another key of the
/// same object, read right after the define in the constructor, keeps its
/// fact.
#[test]
fn r6defprop_defined_key_not_an_accessor_key() {
    let src = prog(ID).replace(
        "this.witness = 1; }",
        "this.witness = 1; const q: number = this.witness; console.log(q); }",
    );
    let pf = facts(&src);
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "witness" && !s.is_call)
        .collect();
    assert_eq!(s.len(), 2, "{s:?}");
    for x in s {
        assert!(!matches!(x.verdict, Verdict::No(_)), "{x:?}");
    }
}

/// (d) The define runs in the base constructor on `this` of a subclass
/// instance: the subclass's own key keeps its fact.
#[test]
fn r6defprop_subclass_keys_kept() {
    let src = prog(ID).replace(
        "const w: number = b.witness;",
        "const w: number = b.isBone ? 1 : 0;",
    );
    let v = witness(&src, "isBone");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// (e) A define on some paths only: the other keys keep their facts, the
/// defined key is unproven.
#[test]
fn r6defprop_conditional_define() {
    let d = "if (process.argv.length > 9) Object.defineProperty(this, 'k', { value: 1 })";
    let v = witness(&prog(d), "witness");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
    let src = prog(d).replace("(b as any).id", "(b as any).k");
    let v = witness(&src, "k");
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}
