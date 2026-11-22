//! ACCREGION: a getter or setter runs inside the [[Get]] or [[Set]] of the
//! member access that reached it (ECMA-262 10.1.8.1 OrdinaryGet step 7,
//! 10.1.9.2 OrdinarySetWithOwnDescriptor step 7, 7.3.14 Call): what it
//! throws completes that access, so it reaches the try around the access,
//! not the program-wide thrown cell every catch reads.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_async_to_generator(&mut hir);
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The kind of `Caught.value`, which holds what the catch under test saw.
struct Seen {
    top: bool,
    mask: u64,
}

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "top={} mask={:#x}", self.top, self.mask)
    }
}

impl Seen {
    fn mask(&self) -> u64 {
        self.mask
    }
}

fn caught(src: &str) -> (Seen, String) {
    let pf = facts_of(src);
    let k = &field(tmpl(&pf, "Caught"), "value").kind;
    let seen = Seen {
        top: k.top,
        mask: k.mask() as u64,
    };
    (seen, pf.dump_text())
}

const CAUGHT: &str = "class Caught { value: any = null; }
    const box = new Caught();\n";

const UNLOCK: &str = "class Snd {
        _v: any = 0;
        get v(): any { try { if (process.argv[3] === \"y\") throw \"s\"; } catch (e) { throw e; } return this._v; }
        set v(x: any) { try { if (process.argv[3] === \"y\") throw \"s\"; } catch (e) { throw e; } this._v = x; }
    }
    const snd = new Snd();
    function k(s: string) { if (s === \"x\") throw 7; }
    function h() { try { k(process.argv[2]); } catch (e: any) { box.value = e; } }\n";

/// RED: a getter that rethrows what it caught (a string), read inside a
/// try: the string reaches that try only, never the catch of `h`, which
/// sees only its own number.
#[test]
#[ignore = "ACCREGION step 2: a Cons::Load carries no region yet, so a getter it runs still throws into THROWN"]
fn accregion_getter_rethrow_stays_in_its_try() {
    let src = format!(
        "{CAUGHT}{UNLOCK}
        function safe() {{ try {{ console.log(snd.v); }} catch (e) {{ console.log(\"no\"); }} }}
        safe();
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// RED: the same for a setter reached by a store under a computed key.
#[test]
fn accregion_setter_any_key_rethrow_stays_in_its_try() {
    let src = format!(
        "{CAUGHT}{UNLOCK}
        function safe(key: string) {{ try {{ (snd as any)[key] = 1; }} catch (e) {{ console.log(\"no\"); }} }}
        safe(process.argv[4]);
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// KEEP: a getter that throws, read inside a try, reaches that catch.
#[test]
fn accregion_getter_throw_reaches_catch() {
    let src = format!(
        "{CAUGHT}{UNLOCK}
        function g() {{ try {{ console.log(snd.v); }} catch (e: any) {{ box.value = e; }} }}
        g();"
    );
    let (k, dump) = caught(&src);
    assert!(k.top || k.mask() & 2 != 0, "{k:?}\n{dump}");
}

/// KEEP: a setter that throws, under a named and a computed key, inside a
/// try, reaches that catch.
#[test]
fn accregion_setter_throw_reaches_catch() {
    for store in ["snd.v = 1;", "(snd as any)[process.argv[4]] = 1;"] {
        let src = format!(
            "{CAUGHT}{UNLOCK}
            function g() {{ try {{ {store} }} catch (e: any) {{ box.value = e; }} }}
            g();"
        );
        let (k, dump) = caught(&src);
        assert!(k.top || k.mask() & 2 != 0, "{store} {k:?}\n{dump}");
    }
}

/// RED: [[Get]] runs only the getter (10.1.8.1 step 7) and [[Set]] only the
/// setter (10.1.9.2 step 7): a read under a computed key, outside any try,
/// never throws what the setter of its group throws. The setter rethrows a
/// string inside a try of its store; the getter throws nothing.
#[test]
fn accregion_read_never_takes_setter_throw() {
    let src = format!(
        "{CAUGHT}class Vol {{
            _v: any = 0;
            get v(): any {{ return this._v; }}
            set v(x: any) {{ try {{ if (process.argv[3] === \"y\") throw \"s\"; }} catch (e) {{ throw e; }} this._v = x; }}
        }}
        const vol = new Vol();
        function k(s: string) {{ if (s === \"x\") throw 7; }}
        function h() {{ try {{ k(process.argv[2]); }} catch (e: any) {{ box.value = e; }} }}
        function safe(key: string) {{ try {{ (vol as any)[key] = 1; }} catch (e) {{ console.log(\"no\"); }} }}
        safe(process.argv[4]);
        console.log((vol as any)[process.argv[5]]);
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// A throwing setter on `Vol.prototype` and a value defined over its key.
const VOL_SETTER: &str = "class Vol {
        _v: any = 0;
        get v(): any { return this._v; }
        set v(x: any) { if (process.argv[3] === \"y\") throw \"s\"; this._v = x; }
    }
    const vol = new Vol();
    function h() { try { k(process.argv[2]); } catch (e: any) { box.value = e; } }
    function desc(): any { return { value: 1, configurable: true }; }\n";

/// STOREREGION: `Object.defineProperty` with a data descriptor the solver
/// reads (`define_by_data`) is [[DefineOwnProperty]] (ECMA-262 20.1.2.4,
/// 10.1.6.3): it runs no setter, so the catch around the define sees only
/// `k`'s own throw.
#[test]
fn storeregion_define_runs_no_setter() {
    let src = format!(
        "{CAUGHT}{VOL_SETTER}
        function k(s: string) {{ if (s === \"x\") throw 7; Object.defineProperty(vol, \"v\", desc()); }}
        console.log(vol.v);
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// Control: an ordinary store in the same place runs the setter, whose
/// throw completes the store and so reaches the catch around `k`.
#[test]
fn storeregion_set_still_runs_setter() {
    let src = format!(
        "{CAUGHT}{VOL_SETTER}
        function k(s: string) {{ if (s === \"x\") throw 7; (vol as any).v = desc(); }}
        console.log(vol.v);
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 != 0,
        "{k:?}\n{dump}"
    );
}

const PXSET: &str = "const px: any = new Proxy({ v: 0 } as any, {
        set(t: any, k: any, x: any): boolean { if (process.argv[3] === \"y\") throw \"s\"; t[k] = x; return true; },
    });
    function h() { try { k(process.argv[2]); } catch (e: any) { box.value = e; } }\n";

/// RED: a Proxy `set` trap that throws a string, run by a store inside a
/// try (10.5.9 step 7, `? Call( trap, ... )`): the string completes that
/// store and reaches that try only, never the catch of `h`.
#[test]
fn callregion_proxy_set_trap_stays_in_its_try() {
    let src = format!(
        "{CAUGHT}{PXSET}
        function k(s: string) {{ if (s === \"x\") throw 7; }}
        function safe() {{ try {{ px.v = 1; }} catch (e) {{ console.log(\"no\"); }} }}
        safe();
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// Control: the same store inside `k`, under `h`'s try: the trap's throw
/// completes the store and so reaches `h`'s catch, with `k`'s own.
#[test]
fn callregion_proxy_set_trap_throw_reaches_catch() {
    let src = format!(
        "{CAUGHT}{PXSET}
        function k(s: string) {{ if (s === \"x\") throw 7; px.v = 1; }}
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 != 0,
        "{k:?}\n{dump}"
    );
}
