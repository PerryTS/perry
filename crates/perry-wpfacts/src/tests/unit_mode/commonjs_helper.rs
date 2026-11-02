//! esbuild's `__commonJS` helper, assigned inside an init function (oxfmt's
//! acorn bundle): the unit that defines `ct` also calls it, through the
//! module cell, so it reads its own published contract of `ct` back. Its
//! inner closure calls `e` and returns `t.exports`: the sound contract says
//! `e` is called and `t` escapes. A contract check that escaped a new
//! closure context of the inner closure without counting it a change left
//! that context's run unsolved when the previous contract already said so:
//! reading the sound contract gave the narrow one and the narrow one the
//! sound one, and the rounds never ended.

use super::*;

const T: &str = r#"
var ct, ti, et, Li;
var init0 = () => {
  ct = (e, t) => () => (t || e((t = { exports: {} }).exports, t), t.exports);
  ti = ct((m, x) => { x.exports = {}; });
  et = ct((m, x) => { var w = ti(); x.exports = () => w; });
};
export var init = () => { init0(); Li = et(); };
export { Li };
"#;

const M: &str = "import { init, Li } from './t'; init(); console.log(Li);";

#[test]
fn a_helper_reading_its_own_contract_back_reaches_the_sound_fixpoint() {
    let mods: &'static [(&'static str, &'static str)] = &[("t", T), ("m", M)];
    // Before: no fixpoint (the solve runs forever), so the bound is a wait.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(units_of(mods));
    });
    let pf = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("per-unit facts reach a fixpoint");
    assert!(pf.rounds <= 4, "rounds {}", pf.rounds);
    let whole = whole_of(mods);
    // The module object `{ exports: {} }` goes to the callbacks and its
    // `exports` back to the callers: it escapes per unit (the callers are
    // other units), and per unit claims no more than whole program.
    let module = |pf: &ProgramFacts| {
        pf.templates
            .iter()
            .find(|t| t.display.starts_with("t.ts:") && t.fields.iter().any(|f| f.key == "exports"))
            .cloned()
            .unwrap_or_else(|| panic!("module object in\n{}", pf.dump_text()))
    };
    let (u, w) = (module(&pf), module(&whole));
    assert!(
        u.escape.is_some(),
        "t escapes (the sound contract)\n{}",
        pf.dump_text()
    );
    let ux = u.fields.iter().find(|f| f.key == "exports").unwrap();
    let wx = w.fields.iter().find(|f| f.key == "exports").unwrap();
    assert!(!wx.readonly && !ux.readonly, "the callbacks store exports");
}

/// The no-fixpoint report names the top-level fields that differ, nested
/// values included, from the derived `Debug` form.
#[test]
fn the_no_fixpoint_report_names_the_changed_fields() {
    #[allow(dead_code)] // the fields are read through the derived Debug
    #[derive(Debug)]
    struct C {
        params: Vec<u8>,
        reach: Vec<u8>,
        ret: Option<Vec<String>>,
        name: String,
    }
    let a = C { params: vec![1, 5], reach: vec![0, 4], ret: Some(vec!["x".into()]), name: "ct".into() };
    let b = C { params: vec![1, 5], reach: vec![0, 0], ret: Some(vec!["y".into()]), name: "ct".into() };
    assert_eq!(crate::units::driver::changed_fields(&a, &b), ["reach", "ret"]);
    assert!(crate::units::driver::changed_fields(&a, &a).is_empty());
}

/// Babel's `fe` (oxfmt's bundle): a recursive arrow assigned in an init
/// function calls itself through its module cell, so its unit reads its own
/// contract back. The contract says the result is the argument or what the
/// argument reaches under `expression`; replaying that through the
/// stand-in loaded `p.expression` into a node outside the stand-in's run,
/// so the caller's own `a.expression` came back as a marker of no run, the
/// result read as another call's value and escaped, and the next solve
/// published the transfer again. Converges on the transfer, claiming no
/// more than the whole program for the caller's deep field it writes
/// through the result.
#[test]
fn a_recursive_helper_reading_its_own_result_transfer_reaches_a_fixpoint() {
    const T: &str = r#"
var fe;
var init0 = () => { fe = (a) => a.type === "P" ? fe(a.expression) : a; };
export var strip = (x) => { init0(); return fe(x); };
"#;
    const M: &str = "import { strip } from './t'; const o = { type: 'P', expression: { type: 'P', expression: { type: 'I', v: 1 } } }; const r = strip(o); r.v = 2; console.log(o.expression.expression.v);";
    let mods: &'static [(&'static str, &'static str)] = &[("t", T), ("m", M)];
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(units_of(mods));
    });
    let u = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("per-unit facts reach a fixpoint");
    assert!(u.rounds <= 4, "rounds {}", u.rounds);
    let w = whole_of(mods);
    let v = |pf: &ProgramFacts| -> Vec<bool> {
        let mut r: Vec<bool> = pf
            .templates
            .iter()
            .filter(|t| t.display.starts_with("m.ts"))
            .flat_map(|t| t.fields.iter().filter(|f| f.key == "v").map(|f| f.readonly))
            .collect();
        r.sort();
        r
    };
    assert_eq!(v(&u), v(&w), "{}", u.dump_text());
    assert!(v(&u).iter().all(|&ro| !ro), "r.v = 2 writes the deep literal");
}
