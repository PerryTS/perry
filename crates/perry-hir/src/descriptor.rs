//! ECMA-262 6.2.6.5 ToPropertyDescriptor on an object literal the compiler
//! reads whole: the attribute entry (`*` bits) the
//! descriptor gives a NEW key. One definition for the layouts (codegen) and
//! the whole-program facts (a constructor's literal-descriptor birth keys).

use crate::Expr;
use perry_abi::{
    LAYOUT_KEY_ACCESSOR as ACCESSOR, LAYOUT_KEY_HAS_GET as HAS_GET, LAYOUT_KEY_HAS_SET as HAS_SET,
    LAYOUT_KEY_NON_CONFIGURABLE as NON_CONFIGURABLE, LAYOUT_KEY_NON_ENUMERABLE as NON_ENUMERABLE,
    LAYOUT_KEY_NON_WRITABLE as NON_WRITABLE,
};

/// The attribute entry a literal property descriptor with these fields
/// (name, value) gives a NEW key: getter and setter presence, and literal
/// booleans (absent ones default to false). `None` when a field is not one
/// the compiler reads. Shared by prototype images and class families
/// (codegen `layout_families`: a constructor's `Object.defineProperty(this,
/// ...)`) and by the facts' constructor phase (`perry_wpfacts::ctor_phase`).
pub fn literal_descriptor_attrs(fields: &[(&str, &Expr)]) -> Option<u8> {
    let function = |v: &Expr| matches!(v, Expr::Closure { .. } | Expr::FuncRef(_));
    let mut attrs = NON_WRITABLE | NON_ENUMERABLE | NON_CONFIGURABLE;
    let mut accessor = 0u8;
    let mut data = false;
    for &(name, value) in fields {
        let flag = match value {
            Expr::Bool(b) => Some(*b),
            _ => None,
        };
        match name {
            "get" if function(value) => accessor |= ACCESSOR | HAS_GET,
            "set" if function(value) => accessor |= ACCESSOR | HAS_SET,
            "value" => data = true,
            "writable" => {
                data = true;
                if flag? {
                    attrs &= !NON_WRITABLE;
                }
            }
            "enumerable" => {
                if flag? {
                    attrs &= !NON_ENUMERABLE;
                }
            }
            "configurable" => {
                if flag? {
                    attrs &= !NON_CONFIGURABLE;
                }
            }
            _ => return None,
        }
    }
    if accessor != 0 {
        if data {
            return None;
        }
        // An accessor has no [[Writable]].
        return Some(accessor | (attrs & !NON_WRITABLE));
    }
    Some(attrs)
}
