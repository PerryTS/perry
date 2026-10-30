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
