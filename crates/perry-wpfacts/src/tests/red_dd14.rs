//! DEFDATA14: a realm accessor function is the accessor it was read from.
//! r7 (three.js GLTFLoader): any-key descriptor walks hand out the realm
//! accessors of the receivers chains (`Map.prototype.size`, the primitive
//! members); one token stood for all of them, so a call of any one with an
//! argument ran the `__proto__` setter (`[[SetPrototypeOf]]`) on its
//! receiver. Only `Object.prototype.__proto__`s setter relinks
//! (perry-runtime property_ops/common.rs `legacy_dunder_proto_set`); a getter
//! called with an argument ignores it, another setter does not relink.
use super::annexb_known_accessor::check;

fn prog(body: &str) -> String {
    format!(
        r#"
        class Cell {{ witness = 1; }}
        class Other {{ x = 2; }}
        const cell = new Cell();
        const other = new Other();
        {body}
        console.log(cell.witness);
    "#
    )
}

/// `Map.prototype.size` getter called with an argument: a brand-check
/// TypeError at run time, never a relink.
#[test]
fn red_dd14_named_getter_called_with_an_argument_keeps_the_prototype() {
    check(&prog("const g: any = (new Map() as any).__lookupGetter__(\"size\"); g.call(cell, other);"), true);
}

/// The r7 shape: an any-key lookup on a Map (its chain holds `size`; the key
/// never names `__proto__`, DECISIONS 217): no `__proto__` setter.
#[test]
fn red_dd14_any_key_getter_called_with_an_argument_keeps_the_prototype() {
    check(&prog("const g: any = (new Map() as any).__lookupGetter__(process.argv[3]); g.call(cell, other);"), true);
}

/// Control (TRUE case): the `__proto__` setter itself relinks `cell`.
#[test]
fn red_dd14_control_proto_setter_relinks() {
    check(&prog("const d: any = Object.getOwnPropertyDescriptor(Object.prototype, \"__proto__\"); d.set.call(cell, other);"), false);
}

/// Control (TRUE case, Annex B lookup): `__lookupSetter__("__proto__")`.
#[test]
fn red_dd14_control_lookup_proto_setter_relinks() {
    check(&prog("const s: any = ({} as any).__lookupSetter__(\"__proto__\"); s.call(cell, other);"), false);
}

/// Control 2: unknown key on `Object.prototype` through the descriptor.
#[test]
fn red_dd14_control_unknown_key_descriptor_setter() {
    check(&prog("const d: any = Object.getOwnPropertyDescriptor(Object.prototype, process.argv[3]); d.set.call(cell, other);"), false);
}
