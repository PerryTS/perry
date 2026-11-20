//! Method calls named like an `Object.prototype` member on the web target.

use super::run_web_source;

// A method call whose name is also an Object.prototype member (`isPrototypeOf`,
// `hasOwnProperty`) reaches the receiver's method, not that member with no receiver.
// Expected output is Node's.
#[test]
fn an_object_prototype_method_call_keeps_its_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const base: any = { k: 1 };
const child: any = { own: 2 };
Object.setPrototypeOf(child, base);
const typed: { k: number } = { k: 1 };
console.log([base.isPrototypeOf(child), child.isPrototypeOf(base), Object.prototype.isPrototypeOf({}), Array.prototype.isPrototypeOf([]), child.hasOwnProperty("own"), child.hasOwnProperty("k"), typed.hasOwnProperty("k"), child.propertyIsEnumerable("own"), base.toString()].join(","));
"#
        ),
        "true,false,true,true,true,false,true,true,[object Object]"
    );
}

// A boolean receiver reaches Boolean.prototype's chain like a string or number does.
#[test]
fn a_boolean_receiver_reaches_its_prototype_methods() {
    assert_eq!(
        run_web_source(
            r#"
const t: any = true;
const f: any = false;
console.log([t.hasOwnProperty("x"), t.toString(), f.valueOf(), t.propertyIsEnumerable("x")].join(","));
"#
        ),
        "false,true,false,false"
    );
}
