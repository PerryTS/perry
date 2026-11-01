use crate::{
    CoreExtern, CoreFuncType, DefinedType, Enum, Flags, FuncType, FuncTypeId, Interface,
    InterfaceId, ItemKind, ModuleType, ModuleTypeId, Record, Resource, ResourceAlias, ResourceId,
    Type, Types, UsedType, ValueType, Variant, World, WorldId,
};
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use semver::Version;
use std::borrow::Borrow;
use std::fmt;
use std::{collections::HashMap, path::Path, rc::Rc};
use wasmparser::{
    component_types::{self as wasm},
    names::{ComponentName, ComponentNameKind},
    Chunk, Encoding, Parser, Payload, ValidPayload, Validator, WasmFeatures,
};

/// Represents a package key that can be used in associative containers.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageKey {
    /// The name of the package,
    name: String,
    /// The version of the package.
    version: Option<Version>,
}

impl PackageKey {
    /// Constructs a new package key from the given package reference.
    pub fn new(package: &Package) -> Self {
        Self {
            name: package.name.clone(),
            version: package.version.clone(),
        }
    }

    /// Gets the name of the package.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the version of the package.
    pub fn version(&self) -> Option<&Version> {
        self.version.as_ref()
    }
}

impl fmt::Display for PackageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{name}", name = self.name)?;
        if let Some(version) = &self.version {
            write!(f, "@{version}")?;
        }

        Ok(())
    }
}

/// A borrowed package key.
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct BorrowedPackageKey<'a> {
    /// The package name.
    pub name: &'a str,
    /// The optional package version.
    pub version: Option<&'a Version>,
}

impl<'a> BorrowedPackageKey<'a> {
    /// Constructs a new borrowed package key from the given package reference.
    pub fn new(package: &'a Package) -> Self {
        Self {
            name: &package.name,
            version: package.version.as_ref(),
        }
    }

    /// Constructs a new borrowed package key from the given name and version.
    pub fn from_name_and_version(name: &'a str, version: Option<&'a Version>) -> Self {
        Self { name, version }
    }

    /// Converts the borrowed package key into an owned package key.
    pub fn into_owned(self) -> PackageKey {
        PackageKey {
            name: self.name.to_owned(),
            version: self.version.cloned(),
        }
    }
}

impl fmt::Display for BorrowedPackageKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{name}", name = self.name)?;
        if let Some(version) = self.version {
            write!(f, "@{version}")?;
        }

        Ok(())
    }
}

/// A trait implemented by types that can be borrowed as a package key.
pub trait BorrowedKey {
    /// Borrows the key as a borrowed package key.
    fn borrowed_key(&self) -> BorrowedPackageKey<'_>;
}

impl BorrowedKey for PackageKey {
    fn borrowed_key(&self) -> BorrowedPackageKey<'_> {
        BorrowedPackageKey {
            name: &self.name,
            version: self.version.as_ref(),
        }
    }
}

impl BorrowedKey for BorrowedPackageKey<'_> {
    fn borrowed_key(&self) -> BorrowedPackageKey<'_> {
        *self
    }
}

impl<'a> Borrow<dyn BorrowedKey + 'a> for PackageKey {
    fn borrow(&self) -> &(dyn BorrowedKey + 'a) {
        self
    }
}

impl Eq for dyn BorrowedKey + '_ {}

impl PartialEq for dyn BorrowedKey + '_ {
    fn eq(&self, other: &dyn BorrowedKey) -> bool {
        self.borrowed_key().eq(&other.borrowed_key())
    }
}

impl std::hash::Hash for dyn BorrowedKey + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.borrowed_key().hash(state)
    }
}

/// Represents a WebAssembly component model package.
///
/// A package is a binary-encoded WebAssembly component.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Package {
    /// The package name.
    name: String,
    /// The package version.
    version: Option<Version>,
    /// The bytes of the package.
    ///
    /// The bytes are a binary-encoded WebAssembly component.
    #[cfg_attr(feature = "serde", serde(skip))]
    bytes: Vec<u8>,
    /// The type of the represented component.
    ty: WorldId,
    /// The type resulting from instantiating the component.
    instance_type: InterfaceId,
    /// Defined interfaces and worlds (from a WIT package).
    definitions: IndexMap<String, ItemKind>,
}

impl Package {
    /// Gets the package key for the package.
    pub fn key(&self) -> BorrowedPackageKey<'_> {
        BorrowedPackageKey::new(self)
    }

    /// Creates a new package from the given file path.
    ///
    /// The package will populate its types into the provided type collection.
    pub fn from_file(
        name: &str,
        version: Option<&Version>,
        path: impl AsRef<Path>,
        types: &mut Types,
    ) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read `{path}`", path = path.display()))?;
        Self::from_bytes(name, version, bytes, types)
    }

    /// Creates a new package from the given bytes.
    ///
    /// The package will populate its types into the provided type collection.
    pub fn from_bytes(
        name: &str,
        version: Option<&Version>,
        bytes: impl Into<Vec<u8>>,
        types: &mut Types,
    ) -> Result<Self> {
        let bytes = bytes.into();
        if !Parser::is_component(&bytes) {
            bail!("package `{name}` is not a binary-encoded WebAssembly component");
        }

        let mut parser = Parser::new(0);
        let mut parsers = Vec::new();
        let mut validator = Validator::new_with_features(WasmFeatures::all());
        let mut imports = Vec::new();
        let mut exports = Vec::new();

        let mut cur = bytes.as_ref();
        loop {
            match parser.parse(cur, true)? {
                Chunk::Parsed { payload, consumed } => {
                    cur = &cur[consumed..];

                    match validator.payload(&payload)? {
                        ValidPayload::Ok => {
                            // Don't parse any sub-components or sub-modules
                            if !parsers.is_empty() {
                                continue;
                            }

                            match payload {
                                Payload::Version { encoding, .. } => {
                                    assert_eq!(encoding, Encoding::Component);
                                }
                                Payload::ComponentImportSection(s) => {
                                    imports.reserve(s.count() as usize);
                                    for import in s {
                                        imports.push(import?.name.name);
                                    }
                                }
                                Payload::ComponentExportSection(s) => {
                                    exports.reserve(s.count() as usize);
                                    for export in s {
                                        exports.push(export?.name.name);
                                    }
                                }
                                _ => {}
                            }
                        }
                        ValidPayload::Func(_, _) => {}
                        ValidPayload::Parser(next) => {
                            parsers.push(parser);
                            parser = next;
                        }
                        ValidPayload::End(wasm_types) => match parsers.pop() {
                            Some(parent) => parser = parent,
                            None => {
                                let mut converter = TypeConverter::new(types, wasm_types);

                                let imports = imports
                                    .into_iter()
                                    .map(|i| Ok((i.to_string(), converter.import(i)?)))
                                    .collect::<Result<_>>()?;

                                let exports: IndexMap<String, ItemKind> = exports
                                    .into_iter()
                                    .map(|i| Ok((i.to_string(), converter.export(i)?)))
                                    .collect::<Result<_>>()?;

                                let ty = types.add_world(World {
                                    id: None,
                                    uses: Default::default(),
                                    imports,
                                    exports: exports.clone(),
                                });

                                let instance_type = types.add_interface(Interface {
                                    id: None,
                                    uses: Default::default(),
                                    exports,
                                });

                                let definitions = Self::find_definitions(types, ty);

                                return Ok(Self {
                                    name: name.to_owned(),
                                    version: version.map(ToOwned::to_owned),
                                    bytes,
                                    ty,
                                    instance_type,
                                    definitions,
                                });
                            }
                        },
                    }
                }
                Chunk::NeedMoreData(_) => panic!("all data should be present"),
            }
        }
    }

    /// Gets the name of the package.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the version of the package.
    pub fn version(&self) -> Option<&Version> {
        self.version.as_ref()
    }

    /// Gets the bytes of the package.
    ///
    /// The bytes are a binary-encoded WebAssembly component.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Gets the id of the world (i.e. component type) of the package.
    pub fn ty(&self) -> WorldId {
        self.ty
    }

    /// Gets the id of the interface (i.e. instance type) that would
    /// result from instantiating the package.
    pub fn instance_type(&self) -> InterfaceId {
        self.instance_type
    }

    /// Gets the interfaces and worlds defined in this package.
    pub fn definitions(&self) -> &IndexMap<String, ItemKind> {
        &self.definitions
    }

    fn find_definitions(types: &Types, world: WorldId) -> IndexMap<String, ItemKind> {
        // Look for any component type exports that export a component type or instance type
        let exports = &types[world].exports;
        let mut defs = IndexMap::new();
        for (name, kind) in exports {
            if let ItemKind::Type(Type::World(id)) = kind {
                let world = &types[*id];
                if world.exports.len() != 1 {
                    continue;
                }

                // Check if the export name is an interface name
                let (export_name, kind) = world.exports.get_index(0).unwrap();
                match ComponentName::new(export_name, 0).unwrap().kind() {
                    ComponentNameKind::Interface(_) => {}
                    _ => continue,
                }

                match kind {
                    ItemKind::Instance(id) => {
                        defs.insert(name.clone(), ItemKind::Type(Type::Interface(*id)));
                    }
                    ItemKind::Component(id) => {
                        defs.insert(name.clone(), ItemKind::Type(Type::World(*id)));
                    }
                    _ => continue,
                }
            }
        }

        defs
    }
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("bytes", &"...")
            .field("ty", &self.ty)
            .field("instance_type", &self.instance_type)
            .field("definitions", &self.definitions)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// The owner is an interface.
    Interface(InterfaceId),
    /// The owner is a world.
    World(WorldId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Entity {
    /// The entity is a type.
    Type(Type),
    /// The entity is a resource.
    Resource(ResourceId),
}

/// Responsible for converting between wasmparser and wac-parser type
/// representations.
struct TypeConverter<'a> {
    types: &'a mut Types,
    wasm_types: Rc<wasmparser::types::Types>,
    cache: HashMap<wasm::AnyTypeId, Entity>,
    resource_map: HashMap<wasm::ResourceId, ResourceId>,
    owners: HashMap<wasm::ComponentAnyTypeId, (Owner, String)>,
}

impl<'a> TypeConverter<'a> {
    fn new(types: &'a mut Types, wasm_types: wasmparser::types::Types) -> Self {
        Self {
            types,
            wasm_types: Rc::new(wasm_types),
            cache: Default::default(),
            resource_map: Default::default(),
            owners: Default::default(),
        }
    }

    fn import(&mut self, name: &str) -> Result<ItemKind> {
        let wasm_types = self.wasm_types.clone();
        let import = wasm_types.component_item_for_import(name).unwrap();
        self.entity(name, import)
    }

    fn export(&mut self, name: &str) -> Result<ItemKind> {
        let wasm_types = self.wasm_types.clone();
        let export = wasm_types.component_item_for_export(name).unwrap();
        self.entity(name, export)
    }

    fn component_func_type(&mut self, id: wasm::ComponentFuncTypeId) -> Result<FuncTypeId> {
        let key = wasm::AnyTypeId::Component(wasm::ComponentAnyTypeId::Func(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Type(Type::Func(id)) => return Ok(*id),
                _ => panic!("invalid cached type"),
            }
        }

        let wasm_types = self.wasm_types.clone();
        let func_ty = &wasm_types[id];
        let params = func_ty
            .params
            .iter()
            .map(|(name, ty)| Ok((name.to_string(), self.component_val_type(*ty)?)))
            .collect::<Result<_>>()?;

        let result = match func_ty.result {
            Some(ty) => Some(self.component_val_type(ty)?),
            None => None,
        };

        let id = self.types.add_func_type(FuncType {
            params,
            result,
            is_async: func_ty.async_,
        });
        self.cache.insert(key, Entity::Type(Type::Func(id)));
        Ok(id)
    }

    fn module_type(&mut self, id: wasm::ComponentCoreModuleTypeId) -> Result<ModuleTypeId> {
        let key = wasm::AnyTypeId::Core(wasm::ComponentCoreTypeId::Module(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Type(Type::Module(id)) => return Ok(*id),
                _ => panic!("invalid cached type"),
            }
        }

        let module_ty = &self.wasm_types[id];

        let imports = module_ty
            .imports
            .iter()
            .map(|((module, name), ty)| {
                Ok(((module.clone(), name.clone()), self.entity_type(*ty)?))
            })
            .collect::<Result<_>>()?;

        let exports = module_ty
            .exports
            .iter()
            .map(|(name, ty)| Ok((name.clone(), self.entity_type(*ty)?)))
            .collect::<Result<_>>()?;

        let module_id = self.types.add_module_type(ModuleType { imports, exports });
        self.cache
            .insert(key, Entity::Type(Type::Module(module_id)));
        Ok(module_id)
    }

    fn ty(&mut self, name: &str, id: wasm::ComponentAnyTypeId) -> Result<Type> {
        match id {
            wasm::ComponentAnyTypeId::Defined(id) => {
                Ok(Type::Value(self.component_defined_type(id)?))
            }
            wasm::ComponentAnyTypeId::Func(id) => Ok(Type::Func(self.component_func_type(id)?)),
            wasm::ComponentAnyTypeId::Component(id) => {
                Ok(Type::World(self.component_type(None, id)?))
            }
            wasm::ComponentAnyTypeId::Instance(id) => {
                Ok(Type::Interface(self.component_instance_type(None, id)?))
            }
            wasm::ComponentAnyTypeId::Resource(id) => Ok(Type::Resource(self.resource(name, id))),
        }
    }

    fn component_val_type(&mut self, ty: wasm::ComponentValType) -> Result<ValueType> {
        match ty {
            wasm::ComponentValType::Primitive(ty) => Ok(ValueType::Primitive(ty.into())),
            wasm::ComponentValType::Type(id) => Ok(self.component_defined_type(id)?),
        }
    }

    fn component_instance_type(
        &mut self,
        name: Option<&str>,
        id: wasm::ComponentInstanceTypeId,
    ) -> Result<InterfaceId> {
        let key = wasm::AnyTypeId::Component(wasm::ComponentAnyTypeId::Instance(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Type(Type::Interface(id)) => {
                    return Ok(*id);
                }
                _ => panic!("invalid cached type"),
            }
        }

        let wasm_types = self.wasm_types.clone();
        let instance_ty = &wasm_types[id];
        let id = self.types.add_interface(Interface {
            id: name.and_then(|n| n.contains(':').then(|| n.to_owned())),
            uses: Default::default(),
            exports: IndexMap::with_capacity(instance_ty.exports.len()),
        });

        for (name, item) in &instance_ty.exports {
            let export = self.entity(name, item)?;

            if let wasm::ComponentEntityType::Type {
                referenced,
                created,
            } = &item.ty
            {
                self.use_or_own(Owner::Interface(id), name, *referenced, *created);

                // Prevent self-referential ownership of any aliased resources in this interface
                if let ItemKind::Type(Type::Resource(res)) = export {
                    if let Some(ResourceAlias { owner, .. }) = &mut self.types[res].alias {
                        if let Some(owner_id) = owner {
                            if *owner_id == id {
                                *owner = None;
                            }
                        }
                    }
                }
            }

            let prev = self.types[id].exports.insert(name.clone(), export);
            assert!(prev.is_none());
        }

        self.cache.insert(key, Entity::Type(Type::Interface(id)));
        Ok(id)
    }

    fn entity(&mut self, name: &str, item: &wasm::ComponentItem) -> Result<ItemKind> {
        match item.ty {
            wasm::ComponentEntityType::Module(id) => Ok(ItemKind::Module(self.module_type(id)?)),
            wasm::ComponentEntityType::Value(ty) => {
                Ok(ItemKind::Value(self.component_val_type(ty)?))
            }
            wasm::ComponentEntityType::Type { created, .. } => {
                Ok(ItemKind::Type(self.ty(name, created)?))
            }
            wasm::ComponentEntityType::Func(id) => {
                Ok(ItemKind::Func(self.component_func_type(id)?))
            }
            wasm::ComponentEntityType::Instance(id) => {
                // A `(implements "I")` import is given a plain-name label (e.g.
                // `primary`) in the binary, but the interface it implements is
                // carried in the `implements` metadata. Use that interface name
                // as the interface id so it resolves to the named interface
                // rather than the opaque label.
                let iface_name = item.implements.as_deref().unwrap_or(name);
                Ok(ItemKind::Instance(
                    self.component_instance_type(Some(iface_name), id)?,
                ))
            }
            wasm::ComponentEntityType::Component(id) => {
                Ok(ItemKind::Component(self.component_type(Some(name), id)?))
            }
        }
    }

    fn use_or_own(
        &mut self,
        owner: Owner,
        name: &str,
        referenced: wasm::ComponentAnyTypeId,
        created: wasm::ComponentAnyTypeId,
    ) {
        if let Some((other, orig)) = self.find_owner(referenced) {
            match *other {
                Owner::Interface(interface) if owner != *other => {
                    let used = UsedType {
                        interface,
                        name: if name != orig {
                            Some(orig.to_string())
                        } else {
                            None
                        },
                    };

                    // Owner is a different interface, so add a using reference
                    let uses = match owner {
                        Owner::Interface(id) => &mut self.types[id].uses,
                        Owner::World(id) => &mut self.types[id].uses,
                    };

                    uses.insert(name.to_string(), used);
                }
                _ => {}
            }
            return;
        }

        // Take ownership of the entity
        let prev = self.owners.insert(created, (owner, name.to_string()));
        assert!(prev.is_none());
    }

    fn component_type(&mut self, name: Option<&str>, id: wasm::ComponentTypeId) -> Result<WorldId> {
        let key = wasm::AnyTypeId::Component(wasm::ComponentAnyTypeId::Component(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Type(Type::World(id)) => return Ok(*id),
                _ => panic!("invalid cached type"),
            }
        }

        let wasm_types = self.wasm_types.clone();
        let component_ty = &wasm_types[id];
        let id = self.types.add_world(World {
            id: name.and_then(|n| n.contains(':').then(|| n.to_owned())),
            uses: Default::default(),
            imports: IndexMap::with_capacity(component_ty.imports.len()),
            exports: IndexMap::with_capacity(component_ty.exports.len()),
        });

        for (name, item) in &component_ty.imports {
            let import = self.entity(name, item)?;

            if let wasm::ComponentEntityType::Type {
                referenced,
                created,
            } = &item.ty
            {
                self.use_or_own(Owner::World(id), name, *referenced, *created);
            }

            let prev = self.types[id].imports.insert(name.clone(), import);
            assert!(prev.is_none());
        }

        for (name, item) in &component_ty.exports {
            let ty = self.entity(name, item)?;
            let prev = self.types[id].exports.insert(name.clone(), ty);
            assert!(prev.is_none());
        }

        self.cache.insert(key, Entity::Type(Type::World(id)));
        Ok(id)
    }

    fn component_defined_type(&mut self, id: wasm::ComponentDefinedTypeId) -> Result<ValueType> {
        let key = wasm::AnyTypeId::Component(wasm::ComponentAnyTypeId::Defined(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Type(Type::Value(ty)) => return Ok(*ty),
                _ => panic!("invalid cached type"),
            }
        }

        let wasm_types = self.wasm_types.clone();
        let ty = match &wasm_types[id] {
            wasm::ComponentDefinedType::Primitive(ty) => ValueType::Defined(
                self.types
                    .add_defined_type(DefinedType::Alias(ValueType::Primitive((*ty).into()))),
            ),
            wasm::ComponentDefinedType::Record(ty) => {
                let fields = ty
                    .fields
                    .iter()
                    .map(|(name, ty)| Ok((name.as_str().to_owned(), self.component_val_type(*ty)?)))
                    .collect::<Result<_>>()?;

                ValueType::Defined(
                    self.types
                        .add_defined_type(DefinedType::Record(Record { fields })),
                )
            }
            wasm::ComponentDefinedType::Variant(ty) => {
                let cases = ty
                    .cases
                    .iter()
                    .map(|(name, case)| {
                        Ok((
                            name.as_str().to_owned(),
                            case.ty.map(|ty| self.component_val_type(ty)).transpose()?,
                        ))
                    })
                    .collect::<Result<_>>()?;

                ValueType::Defined(
                    self.types
                        .add_defined_type(DefinedType::Variant(Variant { cases })),
                )
            }
            wasm::ComponentDefinedType::List { element, .. } => {
                let ty = self.component_val_type(*element)?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::List(ty)))
            }
            wasm::ComponentDefinedType::Tuple(ty) => {
                let types = ty
                    .types
                    .iter()
                    .map(|ty| self.component_val_type(*ty))
                    .collect::<Result<_>>()?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::Tuple(types)))
            }
            wasm::ComponentDefinedType::Flags(flags) => {
                let flags = flags.iter().map(|flag| flag.as_str().to_owned()).collect();
                ValueType::Defined(
                    self.types
                        .add_defined_type(DefinedType::Flags(Flags(flags))),
                )
            }
            wasm::ComponentDefinedType::Enum(cases) => {
                let cases = cases.iter().map(|case| case.as_str().to_owned()).collect();
                ValueType::Defined(self.types.add_defined_type(DefinedType::Enum(Enum(cases))))
            }
            wasm::ComponentDefinedType::Option { ty, .. } => {
                let ty = self.component_val_type(*ty)?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::Option(ty)))
            }
            wasm::ComponentDefinedType::Result { ok, err, .. } => {
                let ok = ok.map(|ty| self.component_val_type(ty)).transpose()?;
                let err = err.map(|ty| self.component_val_type(ty)).transpose()?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::Result { ok, err }))
            }
            wasm::ComponentDefinedType::Borrow(id) => ValueType::Borrow(
                match self.cache.get(&wasm::AnyTypeId::Component(
                    wasm::ComponentAnyTypeId::Resource(*id),
                )) {
                    Some(Entity::Resource(id)) => *id,
                    _ => panic!("expected a resource"),
                },
            ),
            wasm::ComponentDefinedType::Own(id) => ValueType::Own(
                match self.cache.get(&wasm::AnyTypeId::Component(
                    wasm::ComponentAnyTypeId::Resource(*id),
                )) {
                    Some(Entity::Resource(id)) => *id,
                    _ => panic!("expected a resource"),
                },
            ),
            wasm::ComponentDefinedType::Stream { ty, .. } => {
                let stream = ty.map(|ty| self.component_val_type(ty)).transpose()?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::Stream(stream)))
            }
            wasm::ComponentDefinedType::Future { ty, .. } => {
                let option = ty.map(|ty| self.component_val_type(ty)).transpose()?;
                ValueType::Defined(self.types.add_defined_type(DefinedType::Future(option)))
            }
            wasm::ComponentDefinedType::FixedLengthList {
                element, length, ..
            } => {
                let ty = self.component_val_type(*element)?;
                ValueType::Defined(
                    self.types
                        .add_defined_type(DefinedType::FixedSizeList(ty, *length)),
                )
            }
            wasm::ComponentDefinedType::Map { key, value, .. } => {
                let key_ty = self.component_val_type(*key)?;
                let value_ty = self.component_val_type(*value)?;
                ValueType::Defined(
                    self.types
                        .add_defined_type(DefinedType::Map(key_ty, value_ty)),
                )
            }
        };

        self.cache.insert(key, Entity::Type(Type::Value(ty)));
        Ok(ty)
    }

    fn resource(&mut self, name: &str, id: wasm::AliasableResourceId) -> ResourceId {
        let key = wasm::AnyTypeId::Component(wasm::ComponentAnyTypeId::Resource(id));
        if let Some(ty) = self.cache.get(&key) {
            match ty {
                Entity::Resource(id) => return *id,
                _ => panic!("invalid cached type"),
            }
        }

        // Check if this is an alias of another resource
        if let Some(resource_id) = self.resource_map.get(&id.resource()) {
            let alias_id = self.types.add_resource(Resource {
                name: name.to_owned(),
                alias: Some(ResourceAlias {
                    owner: match self.find_owner(wasm::ComponentAnyTypeId::Resource(id)) {
                        Some((Owner::Interface(id), _)) => Some(*id),
                        _ => None,
                    },
                    source: *resource_id,
                }),
            });
            self.cache.insert(key, Entity::Resource(alias_id));
            return alias_id;
        }

        // Otherwise, this is a new resource
        let resource_id = self.types.add_resource(Resource {
            name: name.to_owned(),
            alias: None,
        });

        self.resource_map.insert(id.resource(), resource_id);
        self.cache.insert(key, Entity::Resource(resource_id));
        resource_id
    }

    fn entity_type(&self, ty: wasmparser::types::EntityType) -> Result<CoreExtern> {
        Ok(match ty {
            wasmparser::types::EntityType::Func(ty) => CoreExtern::Func(self.func_type(ty)?),
            wasmparser::types::EntityType::Table(ty) => ty.try_into()?,
            wasmparser::types::EntityType::Memory(ty) => ty.into(),
            wasmparser::types::EntityType::Global(ty) => ty.try_into()?,
            wasmparser::types::EntityType::Tag(ty) => CoreExtern::Tag(self.func_type(ty)?),
            wasmparser::types::EntityType::FuncExact(_) => {
                todo!("wasmparser::types::EntityType::FuncExact")
            }
        })
    }

    fn func_type(&self, ty: wasmparser::types::CoreTypeId) -> Result<CoreFuncType> {
        let func_ty = self.wasm_types[ty].unwrap_func();
        Ok(CoreFuncType {
            params: func_ty
                .params()
                .iter()
                .copied()
                .map(TryInto::try_into)
                .collect::<Result<_>>()?,
            results: func_ty
                .results()
                .iter()
                .copied()
                .map(TryInto::try_into)
                .collect::<Result<_>>()?,
        })
    }

    fn find_owner(&self, mut id: wasm::ComponentAnyTypeId) -> Option<&(Owner, String)> {
        let mut prev = None;
        while prev.is_none() {
            prev = self.owners.get(&id);
            id = match self.wasm_types.peel_alias(id) {
                Some(next) => next,
                None => break,
            };
        }
        prev
    }
}
