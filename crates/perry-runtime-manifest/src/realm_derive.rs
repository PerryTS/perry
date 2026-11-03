//! Header words of realm births. Every birth reader states the birth facts it
//! read and `perry_abi::realm_header` derives the word; no reader sets a bit.
//!
//! One object fact is not local to its birth: an object that another realm
//! birth names as its `[[Prototype]]` is marked at birth. Object births
//! therefore carry their facts until [`finish`] has seen every declaration.
use crate::realm_json::Value;
use perry_abi::realm_header::{
    array_reserved, closure_reserved, members_need_descriptors, object_reserved, ArrayElements,
    ClosureBirth, Integrity, MemberAttrs, ObjectBirth, ObjectExotic, ObjectLayout, PrototypeMark,
};
use std::collections::{BTreeMap, BTreeSet};

/// The header key under which an object birth carries its facts until
/// [`finish`]. It never reaches the manifest.
const FACTS: &str = "@birth";

fn number(n: u64) -> Value {
    Value::Number(n.to_string())
}

/// The header of an array birth.
pub(super) fn array_header(gc_type: u64, elements: ArrayElements, integrity: Integrity) -> Value {
    Value::Object(BTreeMap::from([
        ("gc_type".into(), number(gc_type)),
        (
            "reserved".into(),
            number(u64::from(array_reserved(elements, integrity))),
        ),
    ]))
}

/// The `_reserved` word of a closure birth.
pub(super) fn closure_word(birth: ClosureBirth) -> u64 {
    u64::from(closure_reserved(birth))
}

/// The descriptor fact of an ordinary holder, read from its wire properties.
pub(super) fn descriptors(properties: &[Value]) -> bool {
    let members = properties
        .iter()
        .map(|p| MemberAttrs {
            symbol: p.get("symbol") == Some(&Value::Bool(true)),
            accessor: p.get("get").is_some() || p.get("set").is_some(),
            attributes: p
                .get("attributes")
                .map(Value::encode)
                .and_then(|a| a.parse().ok())
                .unwrap_or(7),
        })
        .collect::<Vec<_>>();
    members_need_descriptors(&members)
}

/// The header of an object birth, with its facts attached for [`finish`].
pub(super) fn object_header(gc_type: u64, class_id: u64, birth: ObjectBirth) -> Value {
    Value::Object(BTreeMap::from([
        ("gc_type".into(), number(gc_type)),
        ("class_id".into(), number(class_id)),
        ("reserved".into(), number(u64::from(object_reserved(birth)))),
        (FACTS.into(), encode(birth)),
    ]))
}

/// Restate a fact of an object birth that a more specific birth reader
/// knows (the layout a prototype or record allocator owns, a mark at birth).
pub(super) fn restate(
    declaration: &mut Value,
    change: impl FnOnce(&mut ObjectBirth),
) -> Result<(), String> {
    let Some(Value::Object(header)) = (match declaration {
        Value::Object(map) => map.get_mut("header"),
        _ => None,
    }) else {
        return Err("object birth has no header".into());
    };
    let mut birth = decode(header.get(FACTS).ok_or("object birth carries no facts")?)?;
    change(&mut birth);
    header.insert("reserved".into(), number(u64::from(object_reserved(birth))));
    header.insert(FACTS.into(), encode(birth));
    Ok(())
}

/// Mark every ordinary object that another declaration names as its
/// `[[Prototype]]` at birth (`proto_validity` marks it when the realm links
/// the child), derive the final words and drop the carried facts.
pub(super) fn finish(declarations: &mut [Value]) -> Result<(), String> {
    let parents = declarations
        .iter()
        .filter_map(|d| d.get("prototype").and_then(Value::string))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for declaration in declarations.iter_mut() {
        if declaration
            .get("header")
            .and_then(|h| h.get(FACTS))
            .is_none()
        {
            continue;
        }
        let name = declaration
            .get("name")
            .and_then(Value::string)
            .ok_or("missing realm name")?
            .to_owned();
        let classless =
            declaration.get("header").and_then(|h| h.get("class_id")) == Some(&number(0));
        if parents.contains(&name) && classless {
            restate(declaration, |b| b.prototype_mark = PrototypeMark::AtBirth)?;
        }
        let Value::Object(map) = declaration else {
            unreachable!()
        };
        let Some(Value::Object(header)) = map.get_mut("header") else {
            unreachable!()
        };
        header.remove(FACTS);
    }
    Ok(())
}

fn encode(birth: ObjectBirth) -> Value {
    let text = |s: &str| Value::String(s.into());
    Value::Object(BTreeMap::from([
        (
            "exotic".into(),
            text(match birth.exotic {
                ObjectExotic::Ordinary => "ordinary",
                ObjectExotic::NativeNamespace => "native_namespace",
            }),
        ),
        ("null_prototype".into(), Value::Bool(birth.null_prototype)),
        (
            "layout".into(),
            text(match birth.layout {
                ObjectLayout::Unmarked => "unmarked",
                ObjectLayout::PlainOrdinary => "plain_ordinary",
                ObjectLayout::Callable => "callable",
            }),
        ),
        (
            "prototype_mark".into(),
            text(match birth.prototype_mark {
                PrototypeMark::AtBirth => "at_birth",
                PrototypeMark::OnFirstLink => "on_first_link",
            }),
        ),
        ("descriptors".into(), Value::Bool(birth.descriptors)),
        (
            "typed_array_prototype".into(),
            Value::Bool(birth.typed_array_prototype),
        ),
        ("integrity".into(), text(integrity_name(birth.integrity))),
    ]))
}

fn decode(facts: &Value) -> Result<ObjectBirth, String> {
    let text = |key: &str| {
        facts
            .get(key)
            .and_then(Value::string)
            .ok_or_else(|| format!("object birth facts lack {key}"))
    };
    let flag = |key: &str| match facts.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("object birth facts lack {key}")),
    };
    Ok(ObjectBirth {
        exotic: match text("exotic")? {
            "ordinary" => ObjectExotic::Ordinary,
            "native_namespace" => ObjectExotic::NativeNamespace,
            other => return Err(format!("unknown object exotic {other}")),
        },
        null_prototype: flag("null_prototype")?,
        layout: match text("layout")? {
            "unmarked" => ObjectLayout::Unmarked,
            "plain_ordinary" => ObjectLayout::PlainOrdinary,
            "callable" => ObjectLayout::Callable,
            other => return Err(format!("unknown object layout {other}")),
        },
        prototype_mark: match text("prototype_mark")? {
            "at_birth" => PrototypeMark::AtBirth,
            "on_first_link" => PrototypeMark::OnFirstLink,
            other => return Err(format!("unknown prototype mark {other}")),
        },
        descriptors: flag("descriptors")?,
        typed_array_prototype: flag("typed_array_prototype")?,
        integrity: integrity(text("integrity")?)?,
    })
}

fn integrity_name(integrity: Integrity) -> &'static str {
    match integrity {
        Integrity::Extensible => "extensible",
        Integrity::NonExtensible => "non_extensible",
        Integrity::Sealed => "sealed",
        Integrity::Frozen => "frozen",
    }
}

/// An integrity level as declarations spell it.
pub(super) fn integrity(name: &str) -> Result<Integrity, String> {
    Ok(match name {
        "extensible" => Integrity::Extensible,
        "non_extensible" => Integrity::NonExtensible,
        "sealed" => Integrity::Sealed,
        "frozen" => Integrity::Frozen,
        other => return Err(format!("unknown integrity level {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary() -> ObjectBirth {
        ObjectBirth {
            exotic: ObjectExotic::Ordinary,
            null_prototype: false,
            layout: ObjectLayout::Unmarked,
            prototype_mark: PrototypeMark::OnFirstLink,
            descriptors: false,
            typed_array_prototype: false,
            integrity: Integrity::Extensible,
        }
    }

    #[test]
    fn facts_round_trip_and_never_reach_the_manifest() {
        let birth = ObjectBirth {
            null_prototype: true,
            layout: ObjectLayout::Callable,
            integrity: Integrity::Sealed,
            ..ordinary()
        };
        assert_eq!(decode(&encode(birth)).unwrap(), birth);
        let mut ds = vec![Value::Object(BTreeMap::from([
            ("name".into(), Value::String("A".into())),
            ("header".into(), object_header(2, 0, birth)),
        ]))];
        finish(&mut ds).unwrap();
        assert!(ds[0].get("header").unwrap().get(FACTS).is_none());
    }

    #[test]
    fn a_named_prototype_is_marked_at_birth_unless_it_has_a_class() {
        let decl = |name: &str, class: u64, prototype: Option<&str>| {
            let mut map = BTreeMap::from([
                ("name".into(), Value::String(name.into())),
                ("header".into(), object_header(2, class, ordinary())),
            ]);
            if let Some(p) = prototype {
                map.insert("prototype".into(), Value::String(p.into()));
            }
            Value::Object(map)
        };
        let mut ds = vec![
            decl("P", 0, None),
            decl("Q", 9, None),
            decl("C", 0, Some("P")),
        ];
        ds.push(decl("D", 0, Some("Q")));
        finish(&mut ds).unwrap();
        let word = |i: usize| {
            ds[i]
                .get("header")
                .unwrap()
                .get("reserved")
                .unwrap()
                .encode()
        };
        assert_eq!(word(0), "128");
        assert_eq!(word(1), "0");
        assert_eq!(word(2), "0");
    }
}
