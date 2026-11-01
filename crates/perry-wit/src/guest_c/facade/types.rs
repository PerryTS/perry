//! Move signatures into one private interface without inventing resource types.
use super::*;
use wit_parser::{Handle, Type, TypeOwner};

pub(super) fn remap(
    resolve: &mut Resolve,
    interface: InterfaceId,
    mapped: &mut BTreeMap<TypeId, TypeId>,
    ty: Type,
) -> Result<Type> {
    let Type::Id(id) = ty else { return Ok(ty) };
    if let Some(id) = mapped.get(&id) {
        return Ok(Type::Id(*id));
    }
    let source = resolve.types[id].clone();
    // Named interface types are imported aliases, including nominal resources.
    let kind = if matches!(source.owner, TypeOwner::Interface(_)) {
        TypeDefKind::Type(Type::Id(id))
    } else {
        match source.kind {
            TypeDefKind::Type(Type::Id(next)) => {
                let result = remap(resolve, interface, mapped, Type::Id(next))?;
                let Type::Id(result) = result else {
                    unreachable!()
                };
                mapped.insert(id, result);
                return Ok(Type::Id(result));
            }
            TypeDefKind::Record(mut record) => {
                for field in &mut record.fields {
                    field.ty = remap(resolve, interface, mapped, field.ty)?;
                }
                TypeDefKind::Record(record)
            }
            TypeDefKind::Tuple(mut tuple) => {
                for ty in &mut tuple.types {
                    *ty = remap(resolve, interface, mapped, *ty)?;
                }
                TypeDefKind::Tuple(tuple)
            }
            TypeDefKind::Variant(mut variant) => {
                for case in &mut variant.cases {
                    case.ty = case
                        .ty
                        .map(|ty| remap(resolve, interface, mapped, ty))
                        .transpose()?;
                }
                TypeDefKind::Variant(variant)
            }
            TypeDefKind::Result(mut result) => {
                result.ok = result
                    .ok
                    .map(|ty| remap(resolve, interface, mapped, ty))
                    .transpose()?;
                result.err = result
                    .err
                    .map(|ty| remap(resolve, interface, mapped, ty))
                    .transpose()?;
                TypeDefKind::Result(result)
            }
            TypeDefKind::Handle(handle) => {
                let (owned, resource) = match handle {
                    Handle::Own(id) => (true, id),
                    Handle::Borrow(id) => (false, id),
                };
                let Type::Id(resource) = remap(resolve, interface, mapped, Type::Id(resource))?
                else {
                    unreachable!()
                };
                TypeDefKind::Handle(if owned {
                    Handle::Own(resource)
                } else {
                    Handle::Borrow(resource)
                })
            }
            TypeDefKind::List(ty) => TypeDefKind::List(remap(resolve, interface, mapped, ty)?),
            TypeDefKind::Option(ty) => TypeDefKind::Option(remap(resolve, interface, mapped, ty)?),
            TypeDefKind::Future(ty) => TypeDefKind::Future(
                ty.map(|ty| remap(resolve, interface, mapped, ty))
                    .transpose()?,
            ),
            TypeDefKind::Stream(ty) => TypeDefKind::Stream(
                ty.map(|ty| remap(resolve, interface, mapped, ty))
                    .transpose()?,
            ),
            TypeDefKind::Type(ty) => TypeDefKind::Type(ty),
            TypeDefKind::Enum(e) => TypeDefKind::Enum(e),
            TypeDefKind::Flags(f) => TypeDefKind::Flags(f),
            TypeDefKind::Resource => anyhow::bail!("A facade cannot invent a world resource"),
            TypeDefKind::Map(..) | TypeDefKind::FixedLengthList(..) | TypeDefKind::Unknown => {
                anyhow::bail!("Unsupported WASI 0.3 facade type")
            }
        }
    };
    let name = format!("perry-type-{}", id.index());
    let handle = matches!(kind, TypeDefKind::Handle(_));
    let new = resolve.types.alloc(wit_parser::TypeDef {
        name: (!handle).then(|| name.clone()),
        kind,
        owner: if handle {
            TypeOwner::None
        } else {
            TypeOwner::Interface(interface)
        },
        docs: Default::default(),
        stability: Default::default(),
        span: Default::default(),
        external_id: None,
    });
    if !handle {
        resolve.interfaces[interface].types.insert(name, new);
    }
    mapped.insert(id, new);
    Ok(Type::Id(new))
}
