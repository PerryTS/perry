//! Internal transport values. WIT remains the type authority: an untyped JSON
//! number is never accepted for s64/u64, and every aggregate is checked against
//! the component's actual canonical type before entering guest code.
#[cfg(test)]
use crate::resources::Resources;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
#[cfg(test)]
use wasmtime::component::ResourceType;
use wasmtime::component::{ResourceAny, Type, Val};

#[cfg(test)]
pub(crate) fn lower(
    value: &Value,
    ty: &Type,
    resources: &Resources,
    transfers: &mut BTreeSet<String>,
) -> Result<Val> {
    lower_with(value, ty, &mut |value, ty, owned| {
        let id = value["$resource"]
            .as_str()
            .context("expected component resource")?;
        resources.get(id, ty, owned, transfers)
    })
}

#[cfg(test)]
pub(crate) fn lower_with(
    value: &Value,
    ty: &Type,
    resource: &mut impl FnMut(&Value, ResourceType, bool) -> Result<ResourceAny>,
) -> Result<Val> {
    lower_typed(value, ty, &mut |value, ty| match ty {
        Type::Own(r) | Type::Borrow(r) => {
            resource(value, *r, matches!(ty, Type::Own(_))).map(Val::Resource)
        }
        _ => bail!("unsupported managed value"),
    })
}
pub(crate) fn lower_typed(
    value: &Value,
    ty: &Type,
    managed: &mut impl FnMut(&Value, &Type) -> Result<Val>,
) -> Result<Val> {
    lower_at(value, ty, managed, 0)
}

fn lower_at(
    value: &Value,
    ty: &Type,
    managed: &mut impl FnMut(&Value, &Type) -> Result<Val>,
    depth: usize,
) -> Result<Val> {
    ensure!(depth < 128, "component value nesting exceeds 128");
    let mut nested = |value: &Value, ty: &Type| lower_at(value, ty, managed, depth + 1);
    macro_rules! integer {
        ($name:ident, $native:ty, $read:ident) => {
            Val::$name(
                <$native>::try_from(value.$read().context("expected an integer")?)
                    .context("integer outside WIT range")?,
            )
        };
    }
    Ok(match ty {
        Type::Bool => Val::Bool(value.as_bool().context("expected bool")?),
        Type::U8 => integer!(U8, u8, as_u64),
        Type::U16 => integer!(U16, u16, as_u64),
        Type::U32 => integer!(U32, u32, as_u64),
        Type::S8 => integer!(S8, i8, as_i64),
        Type::S16 => integer!(S16, i16, as_i64),
        Type::S32 => integer!(S32, i32, as_i64),
        Type::U64 => Val::U64(
            value["$bigint"]
                .as_str()
                .context("u64 requires BigInt")?
                .parse()
                .context("u64 outside WIT range")?,
        ),
        Type::S64 => Val::S64(
            value["$bigint"]
                .as_str()
                .context("s64 requires BigInt")?
                .parse()
                .context("s64 outside WIT range")?,
        ),
        Type::Float32 => Val::Float32(float(value)? as f32),
        Type::Float64 => Val::Float64(float(value)?),
        Type::String => Val::String(value.as_str().context("expected string")?.into()),
        Type::Char => {
            let mut chars = value.as_str().context("expected char")?.chars();
            let c = chars
                .next()
                .context("char must contain one Unicode scalar")?;
            ensure!(
                chars.next().is_none(),
                "char must contain one Unicode scalar"
            );
            Val::Char(c)
        }
        Type::List(list) => Val::List(
            value
                .as_array()
                .context("expected list")?
                .iter()
                .map(|v| nested(v, &list.ty()))
                .collect::<Result<_>>()?,
        ),
        Type::Record(record) => {
            let object = value.as_object().context("expected record")?;
            let fields: Vec<_> = record.fields().collect();
            ensure!(fields.len() == object.len(), "record field count mismatch");
            Val::Record(
                fields
                    .iter()
                    .map(|f| {
                        Ok((
                            f.name.to_string(),
                            nested(
                                object
                                    .get(f.name)
                                    .with_context(|| format!("missing field {}", f.name))?,
                                &f.ty,
                            )?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            )
        }
        Type::Tuple(tuple) => {
            let values = value.as_array().context("expected tuple")?;
            let types: Vec<_> = tuple.types().collect();
            ensure!(values.len() == types.len(), "tuple length mismatch");
            Val::Tuple(
                values
                    .iter()
                    .zip(types.iter())
                    .map(|(v, t)| nested(v, t))
                    .collect::<Result<_>>()?,
            )
        }
        Type::Enum(enumeration) => {
            let name = value.as_str().context("expected enum case")?;
            ensure!(
                enumeration.names().any(|n| n == name),
                "unknown enum case {name}"
            );
            Val::Enum(name.to_string())
        }
        Type::Flags(flags) => {
            let names: Vec<_> = value
                .as_array()
                .context("expected flags")?
                .iter()
                .map(|v| v.as_str().context("flag must be a string"))
                .collect::<Result<_>>()?;
            let allowed: BTreeSet<_> = flags.names().collect();
            let selected: BTreeSet<_> = names.iter().copied().collect();
            ensure!(
                selected.len() == names.len() && selected.is_subset(&allowed),
                "invalid or duplicate flag"
            );
            Val::Flags(names.into_iter().map(str::to_string).collect())
        }
        Type::Variant(variant) => {
            let tag = tag(value)?;
            let case = variant
                .cases()
                .find(|c| c.name == tag)
                .context("unknown variant case")?;
            Val::Variant(
                tag.to_string(),
                payload(value, case.ty.as_ref(), &mut nested)?,
            )
        }
        Type::Option(option) => Val::Option(match tag(value)? {
            "none" => {
                payload(value, None, &mut nested)?;
                None
            }
            "some" => payload(value, Some(&option.ty()), &mut nested)?,
            _ => bail!("option tag must be none or some"),
        }),
        Type::Result(result) => Val::Result(match tag(value)? {
            "ok" => Ok(payload(value, result.ok().as_ref(), &mut nested)?),
            "err" => Err(payload(value, result.err().as_ref(), &mut nested)?),
            _ => bail!("result tag must be ok or err"),
        }),
        Type::Own(_) | Type::Borrow(_) => {
            ensure!(
                !matches!(ty, Type::Own(_)) || value["owned"] == true,
                "a borrowed resource cannot be transferred as an owner"
            );
            managed(value, ty)?
        }
        Type::Future(_) | Type::Stream(_) => managed(value, ty)?,
        _ => bail!("type is unsupported by the component transport: {ty:?}"),
    })
}

fn tag(value: &Value) -> Result<&str> {
    value["tag"].as_str().context("expected tagged WIT value")
}

fn payload(
    value: &Value,
    ty: Option<&Type>,
    lower: &mut impl FnMut(&Value, &Type) -> Result<Val>,
) -> Result<Option<Box<Val>>> {
    let object = value.as_object().context("expected tagged WIT value")?;
    ensure!(
        object.len() == if ty.is_some() { 2 } else { 1 },
        "tagged value field count mismatch"
    );
    ty.map(|ty| lower(object.get("value").context("missing tagged payload")?, ty).map(Box::new))
        .transpose()
}

fn float(value: &Value) -> Result<f64> {
    if let Some(value) = value.as_f64() {
        return Ok(value);
    }
    Ok(match value["$float"].as_str() {
        Some("nan") => f64::NAN,
        Some("infinity") => f64::INFINITY,
        Some("-infinity") => f64::NEG_INFINITY,
        Some("-0") => -0.0,
        _ => bail!("expected floating-point number"),
    })
}

fn float_wire(value: f64) -> Value {
    if value.is_nan() {
        json!({"$float":"nan"})
    } else if value == f64::INFINITY {
        json!({"$float":"infinity"})
    } else if value == f64::NEG_INFINITY {
        json!({"$float":"-infinity"})
    } else if value == 0.0 && value.is_sign_negative() {
        json!({"$float":"-0"})
    } else {
        json!(value)
    }
}

#[cfg(test)]
pub(crate) fn lift(value: &Val, resources: &mut Resources) -> Result<Value> {
    lift_with(value, &mut |value| {
        Ok(json!({"$resource":resources.insert(value),"owned":value.owned()}))
    })
}

pub(crate) fn lift_with(
    value: &Val,
    resource: &mut impl FnMut(ResourceAny) -> Result<Value>,
) -> Result<Value> {
    let tagged = |tag: &str, value: Option<Value>| match value {
        Some(value) => json!({"tag":tag,"value":value}),
        None => json!({"tag":tag}),
    };
    Ok(match value {
        Val::Bool(v) => json!(v),
        Val::U8(v) => json!(v),
        Val::U16(v) => json!(v),
        Val::U32(v) => json!(v),
        Val::S8(v) => json!(v),
        Val::S16(v) => json!(v),
        Val::S32(v) => json!(v),
        Val::U64(v) => json!({"$bigint":v.to_string()}),
        Val::S64(v) => json!({"$bigint":v.to_string()}),
        Val::Float32(v) => float_wire(*v as f64),
        Val::Float64(v) => float_wire(*v),
        Val::String(v) | Val::Enum(v) => json!(v),
        Val::Char(v) => json!(v.to_string()),
        Val::List(v) | Val::Tuple(v) => Value::Array(
            v.iter()
                .map(|v| lift_with(v, resource))
                .collect::<Result<_>>()?,
        ),
        Val::Record(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), lift_with(v, resource)?)))
                .collect::<Result<_>>()?,
        ),
        Val::Variant(tag, v) => tagged(
            tag,
            v.as_deref().map(|v| lift_with(v, resource)).transpose()?,
        ),
        Val::Option(v) => tagged(
            if v.is_some() { "some" } else { "none" },
            v.as_deref().map(|v| lift_with(v, resource)).transpose()?,
        ),
        Val::Result(v) => match v {
            Ok(v) => tagged(
                "ok",
                v.as_deref().map(|v| lift_with(v, resource)).transpose()?,
            ),
            Err(v) => tagged(
                "err",
                v.as_deref().map(|v| lift_with(v, resource)).transpose()?,
            ),
        },
        Val::Flags(v) => json!(v),
        Val::Resource(v) => resource(*v)?,
        _ => bail!("value is unsupported by the synchronous P2 transport"),
    })
}

/// The actual instantiated type is required for async handles at every depth.
pub(crate) fn lift_typed(
    value: &Val,
    ty: &Type,
    managed: &mut impl FnMut(&Val, &Type) -> Result<Value>,
) -> Result<Value> {
    fn at(
        value: &Val,
        ty: &Type,
        managed: &mut impl FnMut(&Val, &Type) -> Result<Value>,
        depth: usize,
    ) -> Result<Value> {
        ensure!(depth < 128, "component value nesting exceeds 128");
        let mut nested = |v: &Val, t: &Type| at(v, t, managed, depth + 1);
        Ok(match (value, ty) {
            (Val::Resource(_), Type::Own(_) | Type::Borrow(_))
            | (Val::Future(_), Type::Future(_))
            | (Val::Stream(_), Type::Stream(_)) => managed(value, ty)?,
            (Val::List(values), Type::List(list)) => Value::Array(
                values
                    .iter()
                    .map(|v| nested(v, &list.ty()))
                    .collect::<Result<_>>()?,
            ),
            (Val::Tuple(values), Type::Tuple(tuple)) => Value::Array(
                values
                    .iter()
                    .zip(tuple.types())
                    .map(|(v, t)| nested(v, &t))
                    .collect::<Result<_>>()?,
            ),
            (Val::Record(values), Type::Record(record)) => Value::Object(
                values
                    .iter()
                    .zip(record.fields())
                    .map(|((name, v), field)| {
                        ensure!(name == field.name, "canonical record name mismatch");
                        Ok((name.clone(), nested(v, &field.ty)?))
                    })
                    .collect::<Result<_>>()?,
            ),
            (Val::Option(value), Type::Option(option)) => match value {
                Some(v) => json!({"tag":"some","value":nested(v,&option.ty())?}),
                None => json!({"tag":"none"}),
            },
            (Val::Result(value), Type::Result(result)) => {
                let (tag, value, ty) = match value {
                    Ok(v) => ("ok", v, result.ok()),
                    Err(v) => ("err", v, result.err()),
                };
                match (value, ty) {
                    (Some(v), Some(t)) => json!({"tag":tag,"value":nested(v,&t)?}),
                    (None, None) => json!({"tag":tag}),
                    _ => bail!("canonical result payload mismatch"),
                }
            }
            (Val::Variant(tag, value), Type::Variant(variant)) => {
                let case = variant
                    .cases()
                    .find(|c| c.name == tag)
                    .context("unknown canonical variant")?;
                match (value, case.ty) {
                    (Some(v), Some(t)) => json!({"tag":tag,"value":nested(v,&t)?}),
                    (None, None) => json!({"tag":tag}),
                    _ => bail!("canonical variant payload mismatch"),
                }
            }
            _ => lift_with(value, &mut |_| {
                bail!("canonical managed value/type mismatch")
            })?,
        })
    }
    at(value, ty, managed, 0)
}
