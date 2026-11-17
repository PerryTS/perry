//! Cross-unit direct callees get `available_externally` bodies in the
//! caller's unit (`module/inline_copies.rs`); everything else stays a
//! declaration, and every function keeps exactly one emitted definition.

use crate::module::LlModule;
use crate::types::{DOUBLE, PTR};

fn define_calling(m: &mut LlModule, name: &str, callees: &[&str]) {
    let f = m.define_function(name, DOUBLE, vec![]);
    let e = f.create_block("entry");
    let mut last = "0.0".to_string();
    for callee in callees {
        last = e.call(DOUBLE, callee, &[]);
    }
    e.ret(DOUBLE, &last);
}

fn unit_defining<'a>(units: &'a [String], name: &str) -> &'a str {
    let header = format!("define double @{name}(");
    let mut homes = units.iter().filter(|u| u.contains(&header));
    let home = homes
        .next()
        .unwrap_or_else(|| panic!("{name} has no definition"));
    assert!(
        homes.next().is_none(),
        "{name} is emitted in more than one unit"
    );
    home
}

fn copied(unit: &str, name: &str) -> bool {
    unit.contains(&format!("define available_externally double @{name}("))
}

fn declared(unit: &str, name: &str) -> bool {
    unit.contains(&format!("declare double @{name}("))
}

#[test]
fn unit_locality_small_cross_unit_callee_gets_available_externally_body() {
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    define_calling(&mut m, "perry_fn_m__caller", &["perry_fn_m__small"]);
    define_calling(&mut m, "perry_fn_m__small", &[]);
    let units = m.render_codegen_units(2);
    assert_eq!(units.len(), 2);
    let caller = unit_defining(&units, "perry_fn_m__caller");
    let home = unit_defining(&units, "perry_fn_m__small");
    assert!(!std::ptr::eq(caller, home), "the fixture needs two units");
    assert!(copied(caller, "perry_fn_m__small"), "{caller}");
    assert!(!declared(caller, "perry_fn_m__small"), "{caller}");
    assert!(!copied(home, "perry_fn_m__caller"), "{home}");
}

#[test]
fn unit_locality_large_noinline_and_address_only_callees_stay_declarations() {
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    m.declare_function("perry_ext_d", DOUBLE, &[]);
    m.declare_function("perry_ext_take", DOUBLE, &[PTR]);
    let big_body = vec!["perry_ext_d"; super::inline_copies::MAX_COPY_INSTRUCTIONS + 1];
    let f = m.define_function("perry_fn_m__caller", DOUBLE, vec![]);
    let e = f.create_block("entry");
    e.call(DOUBLE, "perry_fn_m__big", &[]);
    e.call(DOUBLE, "perry_fn_m__pinned", &[]);
    let r = e.call(DOUBLE, "perry_ext_take", &[(PTR, "@perry_fn_m__taken")]);
    e.ret(DOUBLE, &r);
    define_calling(&mut m, "perry_fn_m__big", &big_body);
    define_calling(&mut m, "perry_fn_m__pinned", &[]);
    m.functions.last_mut().unwrap().no_inline = true;
    define_calling(&mut m, "perry_fn_m__taken", &[]);
    let units = m.render_codegen_units(2);
    let caller = unit_defining(&units, "perry_fn_m__caller");
    for name in ["perry_fn_m__big", "perry_fn_m__pinned", "perry_fn_m__taken"] {
        unit_defining(&units, name);
        assert!(!copied(caller, name), "{name} copied:\n{caller}");
    }
}

#[test]
fn unit_locality_copy_brings_its_own_cross_unit_callees() {
    // caller -> mid -> leaf: wherever `mid` is copied, `leaf` is there too
    // (or defined there), so inlining `mid` never leaves a real call that
    // `mid`'s home unit would have inlined.
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    define_calling(&mut m, "perry_fn_m__caller", &["perry_fn_m__mid"]);
    define_calling(&mut m, "perry_fn_m__mid", &["perry_fn_m__leaf"]);
    define_calling(&mut m, "perry_fn_m__leaf", &[]);
    for n in [2, 3] {
        let units = m.render_codegen_units(n);
        for unit in &units {
            if copied(unit, "perry_fn_m__mid") {
                let leaf_home = unit.contains("define double @perry_fn_m__leaf(");
                assert!(leaf_home || copied(unit, "perry_fn_m__leaf"), "{unit}");
            }
        }
        for name in ["perry_fn_m__caller", "perry_fn_m__mid", "perry_fn_m__leaf"] {
            unit_defining(&units, name);
        }
    }
}
