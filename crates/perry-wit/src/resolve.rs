use crate::{
    Case, Definition, Field, Function, FunctionKind, Interface, Type, World, SCHEMA_VERSION,
};
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use wit_parser::{Resolve, TypeDefKind, TypeId, WorldId, WorldItem};
mod validation;

pub fn parse_file(path: &Path, world: Option<&str>) -> Result<World> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(path)?;
    let world = resolve.select_world(&[package], world)?;
    // Match the maintained C backend before assigning manifest/factory IDs.
    // It clones interfaces used as both imports and exports.
    resolve.generate_nominal_type_ids(world);
    convert(&resolve, world)
}

pub fn parse(source: &str, world: Option<&str>) -> Result<World> {
    let mut resolve = Resolve::default();
    let package = resolve.push_str("component.wit", source)?;
    let world = resolve.select_world(&[package], world)?;
    // Match the maintained C backend before assigning manifest/factory IDs.
    // It clones interfaces used as both imports and exports.
    resolve.generate_nominal_type_ids(world);
    convert(&resolve, world)
}

pub(super) fn resource_id(resolve: &Resolve, mut id: TypeId) -> TypeId {
    while let TypeDefKind::Type(wit_parser::Type::Id(next)) = resolve.types[id].kind {
        id = next;
    }
    id
}

pub(crate) fn convert(resolve: &Resolve, id: WorldId) -> Result<World> {
    let world = &resolve.worlds[id];
    let name = world.package.map_or_else(
        || world.name.clone(),
        |package| resolve.id_of_name(package, &world.name),
    );
    let mut converter = Converter {
        resolve,
        types: BTreeMap::new(),
        export_types: false,
        retained_imports: wit_bindgen_c::imported_types_used_by_exported_interfaces(resolve, id),
    };
    let imports = converter.interfaces(world.imports.iter(), false)?;
    let exports = converter.interfaces(world.exports.iter(), true)?;
    validation::endpoints(&converter.types)?;
    Ok(World {
        schema_version: SCHEMA_VERSION,
        name,
        imports,
        exports,
        types: converter.types,
    })
}

struct Converter<'a> {
    resolve: &'a Resolve,
    types: BTreeMap<String, Definition>,
    export_types: bool,
    retained_imports: HashSet<TypeId>,
}

impl Converter<'_> {
    fn interfaces<'a>(
        &mut self,
        items: impl Iterator<Item = (&'a wit_parser::WorldKey, &'a WorldItem)>,
        exported: bool,
    ) -> Result<Vec<Interface>> {
        let mut interfaces = Vec::new();
        let mut root = Interface {
            name: String::new(),
            functions: Vec::new(),
            resources: BTreeMap::new(),
        };
        for (key, item) in items {
            // Match the canonical backend: root functions use imported types;
            // exported interfaces introduce distinct nominal definitions.
            self.export_types = exported && matches!(item, WorldItem::Interface { .. });
            match item {
                WorldItem::Interface { id, .. } => {
                    let interface = &self.resolve.interfaces[*id];
                    let name = self.resolve.name_world_key(key);
                    let mut functions = Vec::new();
                    let mut resources = BTreeMap::new();
                    for (name, id) in &interface.types {
                        self.definition(*id)?;
                        let resource = resource_id(self.resolve, *id);
                        if matches!(self.resolve.types[resource].kind, TypeDefKind::Resource) {
                            resources.insert(name.clone(), self.definition(resource)?);
                        }
                    }
                    for function in interface.functions.values() {
                        functions.push(
                            self.function(function)
                                .with_context(|| format!("interface {name}"))?,
                        );
                    }
                    interfaces.push(Interface {
                        name,
                        functions,
                        resources,
                    });
                }
                WorldItem::Function(function) => root.functions.push(self.function(function)?),
                WorldItem::Type { id, .. } => {
                    self.definition(*id)?;
                }
            }
        }
        if !root.functions.is_empty() {
            interfaces.insert(0, root);
        }
        Ok(interfaces)
    }

    fn function(&mut self, function: &wit_parser::Function) -> Result<Function> {
        use wit_parser::FunctionKind as Kind;
        let kind = match function.kind {
            Kind::Freestanding | Kind::AsyncFreestanding => FunctionKind::Function,
            Kind::Constructor(id) => FunctionKind::Constructor(self.definition(id)?),
            Kind::Method(id) | Kind::AsyncMethod(id) => FunctionKind::Method(self.definition(id)?),
            Kind::Static(id) | Kind::AsyncStatic(id) => FunctionKind::Static(self.definition(id)?),
        };
        let params = function
            .params
            .iter()
            .map(|param| {
                Ok(Field {
                    name: param.name.clone(),
                    ty: self.ty(param.ty)?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Function {
            name: function.name.clone(),
            params,
            result: function.result.map(|ty| self.ty(ty)).transpose()?,
            kind,
            async_: function.kind.is_async(),
        })
    }

    fn ty(&mut self, ty: wit_parser::Type) -> Result<Type> {
        use wit_parser::Type as T;
        Ok(match ty {
            T::Bool => Type::Bool,
            T::U8 => Type::U8,
            T::U16 => Type::U16,
            T::U32 => Type::U32,
            T::U64 => Type::U64,
            T::S8 => Type::S8,
            T::S16 => Type::S16,
            T::S32 => Type::S32,
            T::S64 => Type::S64,
            T::F32 => Type::F32,
            T::F64 => Type::F64,
            T::Char => Type::Char,
            T::String => Type::String,
            T::Id(id) => Type::Ref(self.definition(id)?),
            T::ErrorContext => bail!("error-context requires the P3 binding profile"),
        })
    }

    fn definition(&mut self, id: TypeId) -> Result<String> {
        let namespace = if self.export_types && !self.retained_imports.contains(&id) {
            "e"
        } else {
            "t"
        };
        let key = format!("{namespace}{}", id.index());
        if self.types.contains_key(&key) {
            return Ok(key);
        }
        let def = &self.resolve.types[id];
        let name = def.name.clone().unwrap_or_else(|| key.clone());
        // Reserve the identity before following references (resource methods
        // and aliases can revisit their owning resource).
        self.types.insert(
            key.clone(),
            Definition {
                name: name.clone(),
                ty: Type::Resource,
            },
        );
        let ty = match &def.kind {
            TypeDefKind::Type(ty) => self.ty(*ty)?,
            TypeDefKind::Resource => Type::Resource,
            TypeDefKind::Handle(wit_parser::Handle::Own(id)) => {
                Type::Own(self.definition(resource_id(self.resolve, *id))?)
            }
            TypeDefKind::Handle(wit_parser::Handle::Borrow(id)) => {
                Type::Borrow(self.definition(resource_id(self.resolve, *id))?)
            }
            TypeDefKind::Record(record) => Type::Record(
                record
                    .fields
                    .iter()
                    .map(|field| {
                        Ok(Field {
                            name: field.name.clone(),
                            ty: self.ty(field.ty)?,
                        })
                    })
                    .collect::<Result<_>>()?,
            ),
            TypeDefKind::Tuple(tuple) => Type::Tuple(
                tuple
                    .types
                    .iter()
                    .map(|ty| self.ty(*ty))
                    .collect::<Result<_>>()?,
            ),
            TypeDefKind::Flags(flags) => {
                // The Component Model binary format requires 1..=32 labels.
                // wit-parser accepts larger source declarations, so reject
                // them before generating bindings that cannot componentize.
                ensure!(
                    (1..=32).contains(&flags.flags.len()),
                    "type {name}: the P2 binding profile requires between 1 and 32 flags"
                );
                Type::Flags(flags.flags.iter().map(|flag| flag.name.clone()).collect())
            }
            TypeDefKind::Enum(enumeration) => Type::Enum(
                enumeration
                    .cases
                    .iter()
                    .map(|case| case.name.clone())
                    .collect(),
            ),
            TypeDefKind::Variant(variant) => Type::Variant(
                variant
                    .cases
                    .iter()
                    .map(|case| {
                        Ok(Case {
                            name: case.name.clone(),
                            ty: case.ty.map(|ty| self.ty(ty)).transpose()?,
                        })
                    })
                    .collect::<Result<_>>()?,
            ),
            TypeDefKind::Option(ty) => Type::Option(Box::new(self.ty(*ty)?)),
            TypeDefKind::Result(result) => Type::Result {
                ok: result.ok.map(|ty| self.ty(ty).map(Box::new)).transpose()?,
                err: result.err.map(|ty| self.ty(ty).map(Box::new)).transpose()?,
            },
            TypeDefKind::List(ty) => Type::List(Box::new(self.ty(*ty)?)),
            TypeDefKind::Future(ty) => {
                Type::Future(ty.map(|t| self.ty(t).map(Box::new)).transpose()?)
            }
            TypeDefKind::Stream(ty) => {
                let mut payload = *ty;
                while let Some(wit_parser::Type::Id(id)) = payload {
                    if let TypeDefKind::Type(next) = self.resolve.types[id].kind {
                        payload = Some(next);
                    } else {
                        break;
                    }
                }
                ensure!(payload != Some(wit_parser::Type::Char),
                    "type {name}: stream<char> is unsupported; use stream<u8> with an explicit encoding");
                Type::Stream(ty.map(|t| self.ty(t).map(Box::new)).transpose()?)
            }
            unsupported => bail!(
                "type {name}: {} is unsupported by the P2 binding profile",
                unsupported.as_str()
            ),
        };
        self.types.insert(key.clone(), Definition { name, ty });
        Ok(key)
    }
}
