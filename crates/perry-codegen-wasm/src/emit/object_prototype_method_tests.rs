//! Method calls named like an `Object.prototype` member on the web target.

use super::{run_web_source, run_web_source_then};

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

// A getter on a primitive's prototype reads the primitive as its receiver. Perry code
// cannot define a host accessor on Number.prototype, so the host side does.
#[test]
fn a_primitive_prototype_getter_sees_the_primitive() {
    assert_eq!(
        run_web_source_then(
            "",
            r#"
Object.defineProperty(Number.prototype, "own", {
  configurable: true,
  get() { return Number.prototype.valueOf.bind(this); },
});
console.log(__classDispatch(5, "own", []));
delete Number.prototype.own;
"#
        ),
        "5"
    );
}
