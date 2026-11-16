use super::*;

fn keys(v: &[&str]) -> Vec<FamilyKey> {
    v.iter().map(|s| FamilyKey::from(*s)).collect()
}

#[test]
fn a_private_element_never_equals_a_property_name() {
    let spelling = crate::codegen::static_private_class::private_storage_key(7, "#x");
    let private = FamilyKey::Private(spelling.clone());
    assert!(private != spelling.as_str());
    assert!(private != spelling);
    assert_eq!(private.name(), None);
    assert_eq!(private.spelling(), spelling);
    assert!(FamilyKey::from("x") == "x");
    assert!(FamilyKey::Name(spelling.clone()) != private);
}

#[test]
fn a_fixed_state_generator_contributes_its_iterator_result_layout() {
    // The resume body returns raw step values; the runtime births the
    // protocol results on `{value, done}`. Reads of those keys through
    // receivers nothing proves keep their colour slots.
    let mut hir = HirModule::new("generator_results");
    let get = |property: &str| {
        Stmt::Expr(Expr::PropertyGet {
            byte_end: 0,
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(1)),
            property: property.to_string(),
        })
    };
    hir.init = vec![
        Stmt::Expr(Expr::LinkGeneratorPrototype {
            obj: Box::new(Expr::Undefined),
            is_async: false,
            fixed_state: true,
            i32_controls: Vec::new(),
            i1_controls: Vec::new(),
        }),
        get("value"),
        get("done"),
    ];
    let colours = colouring(&[&hir]);
    assert_eq!(colours.colored("value").map(|(_, slot)| slot), Some(0));
    assert_eq!(colours.colored("done").map(|(_, slot)| slot), Some(1));
}

#[test]
fn a_colour_tie_goes_to_the_key_more_layouts_hold() {
    // An enum object's members, each read once and held by one layout,
    // fill the budget; a key read as often but held by thirty layouts
    // (a base class and its subclasses) still gets its colour.
    let names: Vec<String> = (0..MAX_COLOURED_KEYS)
        .map(|i| format!("Kind{i:03}"))
        .collect();
    let mut positions: HashMap<&str, BTreeMap<u32, u32>> = HashMap::new();
    let mut reads: HashMap<String, u64> = HashMap::new();
    for (i, n) in names.iter().enumerate() {
        positions.insert(n.as_str(), BTreeMap::from([(i as u32, 1)]));
        reads.insert(n.clone(), 1);
    }
    positions.insert("spa", BTreeMap::from([(0, 30)]));
    reads.insert("spa".into(), 1);
    positions.insert("data", BTreeMap::from([(1, 1)]));
    reads.insert("data".into(), 9);
    let chosen = colour_candidates(&positions, &reads);
    assert_eq!(chosen.len(), MAX_COLOURED_KEYS);
    assert_eq!(chosen[0], "data");
    assert_eq!(chosen[1], "spa");
    assert!(!chosen.contains(&"Kind126"));
}

#[test]
fn a_subclass_agrees_with_its_base_on_inherited_slots() {
    let raw = vec![
        (
            1,
            0,
            keys(&[
                "pos",
                "end",
                "kind",
                "id",
                "flags",
                "transformFlags",
                "parent",
                "emitNode",
            ]),
            8,
        ),
        (
            2,
            1,
            keys(&[
                "pos",
                "end",
                "kind",
                "id",
                "flags",
                "transformFlags",
                "parent",
                "emitNode",
                "escapedText",
            ]),
            9,
        ),
    ];
    let l = layouts_of(raw, HashMap::new(), ProgramLayouts::default());
    let base = &l.families[&1];
    let sub = &l.families[&2];
    for (k, s) in &base.keys {
        assert_eq!(sub.keys.iter().find(|(x, _)| x == k).unwrap().1, *s, "{k}");
    }
    assert_eq!(sub.slot_count, 9);
    assert!(!l.slots_moved(2));
}

#[test]
fn a_coloured_key_takes_its_free_colour_slot() {
    let raw = vec![
        (1, 0, keys(&["a", "b", "kind"]), 3),
        (2, 1, keys(&["a", "b", "kind", "x", "parent"]), 5),
    ];
    let mut colours = ProgramLayouts::default();
    colours.colored.insert("kind".into(), (0, 0));
    colours.colored.insert("parent".into(), (1, 3));
    let l = layouts_of(raw, HashMap::new(), colours);
    let base = &l.families[&1];
    assert_eq!(
        base.keys,
        vec![("a".into(), 1), ("b".into(), 2), ("kind".into(), 0)]
    );
    let sub = &l.families[&2];
    // Inherited slots agree; `parent` takes its colour 3, `x` slot 4.
    assert_eq!(
        sub.keys,
        vec![
            ("a".into(), 1),
            ("b".into(), 2),
            ("kind".into(), 0),
            ("x".into(), 4),
            ("parent".into(), 3)
        ]
    );
}

#[test]
fn late_keys_take_the_slots_after_the_constructor_keys() {
    let raw = vec![
        (1, 0, keys(&["a", "kind", "late1", "late2"]), 2),
        (
            2,
            1,
            keys(&["a", "kind", "b", "late1", "late2", "late3"]),
            3,
        ),
    ];
    let mut colours = ProgramLayouts::default();
    colours.colored.insert("kind".into(), (0, 0));
    colours.colored.insert("late2".into(), (1, 2));
    let l = layouts_of(raw, HashMap::new(), colours);
    let base = &l.families[&1];
    assert_eq!(
        base.keys,
        vec![
            ("a".into(), 1),
            ("kind".into(), 0),
            ("late1".into(), 3),
            ("late2".into(), 2)
        ]
    );
    assert_eq!(base.own_count, 2);
    let sub = &l.families[&2];
    // `b` cannot keep the parent's late slot 2: constructor keys stay in
    // 0..3 and late keys in 3..6, so `late2` moves off its colour.
    assert_eq!(sub.own_count, 3);
    for (k, s) in &sub.keys[..3] {
        assert!(*s < 3, "{k}");
    }
    for (k, s) in &sub.keys[3..] {
        assert!(*s >= 3, "{k}");
    }
}

fn lower(src: &str) -> HirModule {
    let src = src.to_owned();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "test.ts", &mut cache).unwrap();
            perry_hir::lower_module(&parsed.module, "test", "test.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn a_constructor_lists_its_keys_in_the_class_reservation_order() {
    // `this.b = (this.d = .., 2)` and `this.b = this.d = 7` create `d`
    // before `b`: the reservations (`settle_reservations`), the facts'
    // birth and the runtime all order the keys a, d, b, c, and a proven
    // store at d's slot must find `d` there in the family. A branch
    // store (`if (x) this.k = 9`) is a reservation at its place in the
    // text (`constructor_store_keys`), as the facts' birth lists it.
    let hir = lower(
        "class Later { [k: string]: any; constructor() { this.a = 1; this.b = (this.d = \"early\", 2); this.c = 3; this.d = \"late\"; } }\n\
             class Chain { [k: string]: any; constructor() { this.a = 1; this.b = this.d = 7; this.c = 3; } }\n\
             class Br { [k: string]: any; constructor(x: boolean) { this.a = 1; if (x) this.k = 9; this.b = 2; } }\n",
    );
    let names = |class: &str| -> Vec<String> {
        let c = hir.classes.iter().find(|c| c.name == class).unwrap();
        own_keys(c, false, false, &literal_shapes(hir.classes.iter()))
            .unwrap()
            .keys
            .into_iter()
            .map(|(k, _)| k.name().unwrap().to_string())
            .collect()
    };
    assert_eq!(names("Later"), ["a", "d", "b", "c"]);
    assert_eq!(names("Chain"), ["a", "d", "b", "c"]);
    assert_eq!(names("Br"), ["a", "k", "b"]);
}

/// LIFE-C3: a class's keys are in the order of the constructor's first
/// store of each, whether the store is at top level or in an `if`/`else`
/// arm, a nested branch, a loop body, a `try`, `catch` or `finally`, or
/// one arm only; field definitions come first, a parent's keys before a
/// subclass's. The semver shape (`prerelease` in both arms, then
/// `build`) put `build` first and cost every object an overflow table.
#[test]
fn a_constructor_lists_its_keys_in_first_store_order() {
    let hir = lower(
        "class IfElse { [k: string]: any; constructor(i: number) { this.raw = 1; if (i) { this.pre = []; } else { this.pre = [i]; } this.build = []; } }\n\
             class Nested { [k: string]: any; constructor(a: boolean, b: boolean) { this.x = 1; if (a) { if (b) { this.deep = 2; } else { this.shallow = 3; } this.mid = 4; } this.y = 5; } }\n\
             class Loop { [k: string]: any; constructor(n: number) { this.first = 0; for (let i = 0; i < n; i++) { this.count = i; } while (n-- > 0) { this.w = n; } this.last = n; } }\n\
             class Tried { [k: string]: any; constructor(f: boolean) { this.a = 1; try { if (f) throw 1; this.ok = 1; } catch (e) { this.err = 1; } finally { this.fin = 1; } this.z = 2; } }\n\
             class OneArm { [k: string]: any; constructor(t: boolean) { this.p = 1; if (t) { this.opt = 1; } this.q = 2; } }\n\
             class Fields { [k: string]: any; f1 = 1; f2 = 2; constructor(c: boolean) { if (c) { this.cb = 1; } else { this.cb = 2; } this.top = 3; } }\n\
             class Base { [k: string]: any; constructor(c: boolean) { if (c) { this.bk = 1; } this.bt = 1; } }\n\
             class Sub extends Base { constructor(c: boolean) { super(c); if (c) { this.sk = 1; } else { this.sk = 2; } this.st = 2; } }\n\
             class Exit { [k: string]: any; constructor(c: number) { if (c === 1) { this.r = 1; this.only = 1; return; } if (c === 2) { throw 1; } else { this.e = 1; } this.o = 1; this.r = 2; } }\n",
    );
    let names = |class: &str| -> Vec<String> {
        let c = hir.classes.iter().find(|c| c.name == class).unwrap();
        let has_parent = c.extends_name.is_some();
        own_keys(c, has_parent, false, &literal_shapes(hir.classes.iter()))
            .unwrap()
            .keys
            .into_iter()
            .map(|(k, _)| k.name().unwrap().to_string())
            .collect()
    };
    assert_eq!(names("IfElse"), ["raw", "pre", "build"]);
    assert_eq!(names("Nested"), ["x", "deep", "shallow", "mid", "y"]);
    assert_eq!(names("Loop"), ["first", "count", "w", "last"]);
    assert_eq!(names("Tried"), ["a", "ok", "err", "fin", "z"]);
    assert_eq!(names("OneArm"), ["p", "opt", "q"]);
    assert_eq!(names("Fields"), ["f1", "f2", "cb", "top"]);
    assert_eq!(names("Base"), ["bk", "bt"]);
    assert_eq!(names("Sub"), ["sk", "st"]);
    // An arm that leaves the constructor (`return`, `throw`) fixes no
    // position: the path to the end orders `e, o, r`, and a key only that
    // arm stores follows.
    assert_eq!(names("Exit"), ["e", "o", "r", "only"]);
    // The reservations themselves (the class key index and the facts'
    // birth read them) are in the same order: one list.
    let reserved = |class: &str| -> Vec<String> {
        let c = hir.classes.iter().find(|c| c.name == class).unwrap();
        c.fields
            .iter()
            .filter(|f| f.origin == perry_hir::ClassFieldOrigin::ConstructorStore)
            .map(|f| f.name.clone())
            .collect()
    };
    assert_eq!(reserved("IfElse"), ["raw", "pre", "build"]);
    assert_eq!(reserved("Tried"), ["a", "ok", "err", "fin", "z"]);
    assert_eq!(reserved("Sub"), ["sk", "st"]);
    assert_eq!(reserved("Exit"), ["e", "o", "r", "only"]);
}

/// THR3: a constructor's literal data definition is a family key with
/// the definition's attributes, at its statement position; an all-default
/// one, an accessor, a computed key, a key also stored by `[[Set]]` and a
/// definition in a branch stay plain keys or no keys.
#[test]
fn a_constructor_definition_is_a_family_key_with_its_attributes() {
    let hir = lower(
        "class Dp { [k: string]: any; constructor(v: number) { Object.defineProperty(this, \"id\", { value: v }); this.k = 1; Object.defineProperties(this, { position: { configurable: true, enumerable: true, value: v }, rot: { value: v, writable: true }, all: { value: 1, writable: true, enumerable: true, configurable: true } }); this.name = \"n\"; } }\n\
             class Mixed { [k: string]: any; constructor(f: boolean) { this.a = 1; Object.defineProperty(this, \"a\", { value: 2 }); Object.defineProperty(this, \"g\", { get() { return 1; } }); if (f) Object.defineProperty(this, \"br\", { value: 3 }); this.b = 2; } }\n",
    );
    let keys = |class: &str| -> Vec<(String, u8)> {
        let c = hir.classes.iter().find(|c| c.name == class).unwrap();
        own_keys(c, false, false, &literal_shapes(hir.classes.iter()))
            .unwrap()
            .keys
            .into_iter()
            .map(|(k, _)| (k.name().unwrap().to_string(), k.attrs()))
            .collect()
    };
    let nw = crate::runtime_abi::LAYOUT_KEY_NON_WRITABLE;
    let ne = crate::runtime_abi::LAYOUT_KEY_NON_ENUMERABLE;
    let nc = crate::runtime_abi::LAYOUT_KEY_NON_CONFIGURABLE;
    assert_eq!(
        keys("Dp"),
        [
            ("id".to_string(), nw | ne | nc),
            ("k".to_string(), 0),
            ("position".to_string(), nw),
            ("rot".to_string(), ne | nc),
            ("all".to_string(), 0),
            ("name".to_string(), 0),
        ]
    );
    assert_eq!(keys("Mixed"), [("a".to_string(), 0), ("b".to_string(), 0)]);
}
