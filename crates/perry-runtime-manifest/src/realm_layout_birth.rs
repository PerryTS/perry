//! Prototype allocations derive their header from the runtime layout they own.
use super::{birth, info, tokens, Token};
use crate::realm_json::Value;
use perry_abi::realm_header::{ObjectLayout, PrototypeMark};

fn descriptor(source: &str, name: &str) -> Result<Vec<String>, String> {
    let ts = tokens(source)?;
    for i in 0..ts.len().saturating_sub(4) {
        if ts[i].text == "runtime_layout" && ts[i + 1].text == "!" && ts[i + 3].text == name {
            let end = super::closing_paren(&ts, i + 2)?;
            return Ok(info::split(&ts[i + 3..end], ",")
                .into_iter()
                .map(info::text)
                .collect());
        }
    }
    Err(format!("missing prototype layout {name}"))
}

pub(super) fn prototype(expression: &str, entry: &[Token<'_>]) -> Result<Value, String> {
    let args = expression
        .strip_prefix("crate::object::layout_desc::alloc_layout_prototype(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or("invalid layout prototype allocator")?;
    let ts = tokens(args)?;
    let args = info::split(&ts, ",");
    if args.len() != 2 {
        return Err("prototype allocator requires layout and room".into());
    }
    let layout = info::text(args[0]);
    let name = layout
        .strip_prefix("std::ptr::addr_of!(crate::object::runtime_layouts::")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or("prototype birth requires its existing layout expression")?;
    let source =
        include_str!("../../perry-runtime/src/object/runtime_layouts/builtin_prototypes.rs");
    let fields = descriptor(source, name)?;
    // A seventh field names the realm cells that own the layout (`realm = [..]`).
    let owned = fields.len() == 7 && fields[6].starts_with("realm=");
    if !(fields.len() == 6 || owned) || fields[1].clone().strip_prefix("kind=").is_none() {
        return Err("prototype layout must declare its kind".into());
    }
    let null = fields[4] == "crate::object::layout_desc::LAYOUT_PROTO_NULL";
    let ordinary = fields[4] == "crate::object::layout_desc::LAYOUT_PROTO_OBJECT";
    if !null && !ordinary {
        return Err("unsupported layout prototype inheritance".into());
    }
    let mut d = birth::birth(
        &tokens(if null {
            "js_object_alloc_null_proto(0,0)"
        } else {
            "js_object_alloc(0,0)"
        })?,
        entry,
    )?;
    let kind = fields[1].clone();
    // The layout prototype allocator owns the layout and marks the prototype
    // at birth.
    super::derive::restate(&mut d, |birth| {
        birth.prototype_mark = PrototypeMark::AtBirth;
        birth.layout = match kind.as_str() {
            "kind=LAYOUT_KIND_UNMARKED" => ObjectLayout::Unmarked,
            "kind=LAYOUT_KIND_CALLABLE" => ObjectLayout::Callable,
            _ => ObjectLayout::PlainOrdinary,
        };
    })?;
    let Value::Object(map) = &mut d else {
        unreachable!()
    };
    if kind == "kind=LAYOUT_KIND_CALLABLE" {
        if let Some(bits) = map["properties"]
            .array()
            .unwrap()
            .iter()
            .find(|p| p.get("key").and_then(Value::string) == Some("length"))
            .and_then(|p| p.get("value"))
            .and_then(|v| v.get("value"))
            .and_then(Value::string)
        {
            let bits = u64::from_str_radix(bits, 16).map_err(|e| e.to_string())?;
            map.insert(
                "length".into(),
                Value::Number(f64::from_bits(bits).to_string()),
            );
        }
    }
    Ok(d)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prototype_layout_is_required_and_controls_null_and_callable_flags() {
        for (layout, entry, reserved) in [
            (
                "OBJECT_PROTOTYPE",
                "Object.prototype, members = { constructor; }",
                2240,
            ),
            (
                "FUNCTION_PROTOTYPE",
                "Function.prototype, members = { constructor; }",
                2208,
            ),
        ] {
            let d = prototype(&format!("crate::object::layout_desc::alloc_layout_prototype(std::ptr::addr_of!(crate::object::runtime_layouts::{layout}),0)"), &tokens(entry).unwrap()).unwrap();
            assert_eq!(
                d.get("header").unwrap().get("reserved").unwrap().encode(),
                reserved.to_string()
            );
            assert_eq!(d.get("prototype").is_none(), layout == "OBJECT_PROTOTYPE");
        }
        assert!(descriptor("", "MISSING").is_err());
    }
}
