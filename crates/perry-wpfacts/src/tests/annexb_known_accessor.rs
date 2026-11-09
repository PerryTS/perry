//! B4: an accessor whose function is a known class, builtin or
//! Object.prototype method runs as an ordinary call of that function; only
//! an unknown (⊤) function value stays an opaque accessor.
use super::*;

fn verdict(src: &str) -> Verdict {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert!(!sites.is_empty(), "missing witness site");
    assert_eq!(sites.len(), 1, "{sites:?}");
    sites[0].verdict.clone()
}

fn check(src: &str, proven: bool) {
    let v = verdict(src);
    assert_eq!(!matches!(v, Verdict::No(_)), proven, "{v:?}");
    assert!(
        !matches!(v, Verdict::No(ref s) if s.starts_with("dead site")),
        "{v:?}"
    );
}

/// tsc's debug.ts assertionCache (ANNEXB1 fixture f1): an enumerated key
/// may name an Object.prototype method, so `Debug.assertNode(1, getter)`
/// may be `Debug.__defineGetter__(1, getter)`, an accessor under a computed
/// key on `Debug`. `getter` is the second argument. The witness is a
/// value `Debug` holds by name: it stays known exactly when `Debug` does not
/// escape. (The `Payload` read is not the witness: an Object.prototype
/// method run on `Debug` includes `__lookupGetter__`, whose result on a
/// holder with computed-key stores is ⊤, a rule of its own.)
fn debug_cache(prelude: &str, getter: &str) -> String {
    format!(
        r#"
        {prelude}
        const hasOwn = Object.prototype.hasOwnProperty;
        function getOwnKeys(map: any): string[] {{
          const keys: string[] = [];
          for (const key in map) {{ if (hasOwn.call(map, key)) keys.push(key); }}
          return keys;
        }}
        function noop(_?: unknown): void {{}}
        class Payload {{ n = 1; }}
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        namespace Debug {{
          export const keep = cell;
          let level = 0;
          const assertionCache: any = {{}};
          export function setAssertionLevel(l: number): void {{
            const prev = level; level = l;
            if (l > prev) {{
              for (const key of getOwnKeys(assertionCache)) {{
                const cached = assertionCache[key];
                if (cached !== undefined && l >= cached.level) {{ (Debug as any)[key] = cached; assertionCache[key] = undefined; }}
              }}
            }}
          }}
          function shouldAssertFunction(l: number, name: "assertNode"): boolean {{
            if (level < l) {{ assertionCache[name] = {{ level: l, assertion: (Debug as any)[name] }}; (Debug as any)[name] = noop; return false; }}
            return true;
          }}
          export function assertNode(x: unknown, test: unknown): void {{
            if (shouldAssertFunction(1, "assertNode")) {{ if (!x) throw new Error(String(test)); }}
          }}
          export function payload(): Payload {{ return new Payload(); }}
        }}
        Debug.assertNode(1, {getter});
        Debug.setAssertionLevel(Number(process.argv.length));
        const p = Debug.payload();
        console.log(p.n, cell.witness);
    "#
    )
}

/// The tsc shape itself (f1): the getter is a class. Called as a getter it
/// throws a TypeError before it reads `this`: `Debug` does not escape.
#[test]
fn class_getter_on_the_debug_namespace_keeps_its_members() {
    check(&debug_cache("class Marker {}", "Marker"), true);
}

/// `Object` as a getter returns a fresh object.
#[test]
fn builtin_getter_on_the_debug_namespace_keeps_its_members() {
    check(&debug_cache("", "Object"), true);
}

/// An Object.prototype method as a getter runs on the receiver as the
/// method call does.
#[test]
fn method_getter_on_the_debug_namespace_keeps_its_members() {
    check(&debug_cache("", "Object.prototype.toString"), true);
}

/// Control: an unknown function value stays an opaque accessor (its result
/// is ⊤ and `Debug` escapes).
#[test]
fn unknown_getter_on_the_debug_namespace_stays_opaque() {
    check(
        &debug_cache("declare function unknown(): any;", "unknown()"),
        false,
    );
}

/// Control: a program function getter is still analysed as a call: it hands
/// what its receiver holds to unknown code.
#[test]
fn program_getter_on_the_debug_namespace_runs_its_body() {
    check(
        &debug_cache(
            "declare function external(x: any): void;",
            "function (this: any) { external(this.keep); }",
        ),
        false,
    );
}

/// A class getter read directly: the read throws, the holder's other
/// members stay known.
#[test]
fn class_getter_read_leaves_the_holder_known() {
    check(
        r#"
        class C {}
        class Cell { witness = 1; }
        const o: any = { cell: new Cell() };
        o.__defineGetter__("k", C);
        try { console.log(o.k); } catch (e) {}
        console.log(o.cell.witness);
    "#,
        true,
    );
}

/// `Object` as a getter: the read is a fresh object, not the holder.
#[test]
fn builtin_getter_read_is_a_fresh_object() {
    check(
        r#"
        class Cell { witness = 1; }
        const o: any = { cell: new Cell() };
        o.__defineGetter__("k", Object);
        const r = o.k;
        r.cell = 0;
        console.log(o.cell.witness);
    "#,
        true,
    );
}

/// `Object.prototype.valueOf` as a getter returns the receiver: a store
/// through the read writes the holder.
#[test]
fn method_getter_read_runs_on_the_receiver() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { witness = 1; }
        const o: any = { cell: new Cell() };
        o.__defineGetter__("k", Object.prototype.valueOf);
        const r = o.k;
        r.cell = unknown();
        console.log(o.cell.witness);
    "#,
        false,
    );
}

/// Control for the setter form: a ⊤ setter stays opaque (the holder
/// escapes, what it holds is unknown).
#[test]
fn unknown_setter_stays_opaque() {
    check(
        r#"
        declare function unknown(): any;
        class Cell { witness = 1; }
        const o: any = { cell: new Cell() };
        o.__defineSetter__("k", unknown());
        o.k = 1;
        console.log(o.cell.witness);
    "#,
        false,
    );
}

/// A class setter: the store throws, the holder stays known.
#[test]
fn class_setter_store_leaves_the_holder_known() {
    check(
        r#"
        class C {}
        class Cell { witness = 1; }
        const o: any = { cell: new Cell() };
        o.__defineSetter__("k", C);
        try { o.k = 1; } catch (e) {}
        console.log(o.cell.witness);
    "#,
        true,
    );
}
