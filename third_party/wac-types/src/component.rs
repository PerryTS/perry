use crate::ModuleType;
use id_arena::{Arena, Id};
use indexmap::{IndexMap, IndexSet};
use std::{
    fmt,
    ops::{Index, IndexMut},
};

/// An identifier for defined value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinedTypeId(Id<DefinedType>);

#[cfg(feature = "serde")]
impl serde::Serialize for DefinedTypeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for DefinedTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// An identifier for resource types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(Id<Resource>);

#[cfg(feature = "serde")]
impl serde::Serialize for ResourceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// An identifier for function types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncTypeId(Id<FuncType>);

#[cfg(feature = "serde")]
impl serde::Serialize for FuncTypeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for FuncTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// An identifier for interfaces.
///
/// An interface is analogous to an instance type in the component model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceId(Id<Interface>);

#[cfg(feature = "serde")]
impl serde::Serialize for InterfaceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for InterfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// An identifier for worlds.
///
/// A world is analogous to a component type in the component model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldId(Id<World>);

#[cfg(feature = "serde")]
impl serde::Serialize for WorldId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// An identifier for module types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleTypeId(Id<ModuleType>);

#[cfg(feature = "serde")]
impl serde::Serialize for ModuleTypeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.index().serialize(serializer)
    }
}

impl fmt::Display for ModuleTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

#[cfg(feature = "serde")]
fn serialize_arena<T, S>(arena: &Arena<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: serde::Serialize,
{
    use serde::ser::SerializeSeq;

    let mut s = serializer.serialize_seq(Some(arena.len()))?;
    for (_, e) in arena.iter() {
        s.serialize_element(e)?;
    }

    s.end()
}

/// Represents a component model types collection.
#[derive(Default, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Types {
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    defined: Arena<DefinedType>,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    resources: Arena<Resource>,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    funcs: Arena<FuncType>,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    interfaces: Arena<Interface>,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    worlds: Arena<World>,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_arena"))]
    modules: Arena<ModuleType>,
}

impl Types {
    /// Creates a new types collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Iterates the defined types in the collection.
    pub fn defined_types(&self) -> impl Iterator<Item = &DefinedType> {
        self.defined.iter().map(|(_, t)| t)
    }

    /// Iterates the resources in the collection.
    pub fn resources(&self) -> impl Iterator<Item = &Resource> {
        self.resources.iter().map(|(_, t)| t)
    }

    /// Iterates the function types in the collection.
    pub fn func_types(&self) -> impl Iterator<Item = &FuncType> {
        self.funcs.iter().map(|(_, t)| t)
    }

    /// Iterates the interfaces in the collection.
    pub fn interfaces(&self) -> impl Iterator<Item = &Interface> {
        self.interfaces.iter().map(|(_, t)| t)
    }

    /// Iterates the worlds in the collection.
    pub fn worlds(&self) -> impl Iterator<Item = &World> {
        self.worlds.iter().map(|(_, t)| t)
    }

    /// Iterates the modules in the collection.
    pub fn modules(&self) -> impl Iterator<Item = &ModuleType> {
        self.modules.iter().map(|(_, t)| t)
    }

    /// Adds a defined value type to the collection.
    pub fn add_defined_type(&mut self, ty: DefinedType) -> DefinedTypeId {
        DefinedTypeId(self.defined.alloc(ty))
    }

    /// Adds a resource to the collection.
    pub fn add_resource(&mut self, resource: Resource) -> ResourceId {
        ResourceId(self.resources.alloc(resource))
    }

    /// Adds a function type to the collection.
    pub fn add_func_type(&mut self, func: FuncType) -> FuncTypeId {
        FuncTypeId(self.funcs.alloc(func))
    }

    /// Adds an interface (i.e. instance type) to the collection.
    pub fn add_interface(&mut self, interface: Interface) -> InterfaceId {
        InterfaceId(self.interfaces.alloc(interface))
    }

    /// Adds a world (i.e. component type) to the collection.
    pub fn add_world(&mut self, world: World) -> WorldId {
        WorldId(self.worlds.alloc(world))
    }

    /// Adds a module type to the collection.
    pub fn add_module_type(&mut self, module: ModuleType) -> ModuleTypeId {
        ModuleTypeId(self.modules.alloc(module))
    }

    /// Determines if the given type is defined in this collection.
    ///
    /// Note that primitive types are always considered part of a collection.
    pub fn contains(&self, ty: Type) -> bool {
        match ty {
            Type::Resource(id)
            | Type::Value(ValueType::Borrow(id))
            | Type::Value(ValueType::Own(id)) => self.resources.get(id.0).is_some(),
            Type::Func(id) => self.funcs.get(id.0).is_some(),
            Type::Value(ValueType::Primitive(_)) => true,
            Type::Value(ValueType::Defined(id)) => self.defined.get(id.0).is_some(),
            Type::Interface(id) => self.interfaces.get(id.0).is_some(),
            Type::World(id) => self.worlds.get(id.0).is_some(),
            Type::Module(id) => self.modules.get(id.0).is_some(),
        }
    }

    /// Resolves a value type to a un-aliased value type.
    pub fn resolve_value_type(&self, mut ty: ValueType) -> ValueType {
        loop {
            match ty {
                ValueType::Defined(id) => match self[id] {
                    DefinedType::Alias(aliased) => ty = aliased,
                    _ => return ty,
                },
                _ => return ty,
            }
        }
    }

    /// Resolves any aliased resource id to the underlying defined resource id.
    pub fn resolve_resource(&self, mut id: ResourceId) -> ResourceId {
        while let Some(alias) = &self[id].alias {
            id = alias.source;
        }

        id
    }
}

impl Index<DefinedTypeId> for Types {
    type Output = DefinedType;

    fn index(&self, id: DefinedTypeId) -> &Self::Output {
        &self.defined[id.0]
    }
}

impl Index<ResourceId> for Types {
    type Output = Resource;

    fn index(&self, id: ResourceId) -> &Self::Output {
        &self.resources[id.0]
    }
}

impl Index<FuncTypeId> for Types {
    type Output = FuncType;

    fn index(&self, id: FuncTypeId) -> &Self::Output {
        &self.funcs[id.0]
    }
}

impl Index<InterfaceId> for Types {
    type Output = Interface;

    fn index(&self, id: InterfaceId) -> &Self::Output {
        &self.interfaces[id.0]
    }
}

impl Index<WorldId> for Types {
    type Output = World;

    fn index(&self, id: WorldId) -> &Self::Output {
        &self.worlds[id.0]
    }
}

impl Index<ModuleTypeId> for Types {
    type Output = ModuleType;

    fn index(&self, id: ModuleTypeId) -> &Self::Output {
        &self.modules[id.0]
    }
}

impl IndexMut<DefinedTypeId> for Types {
    fn index_mut(&mut self, id: DefinedTypeId) -> &mut Self::Output {
        &mut self.defined[id.0]
    }
}

impl IndexMut<ResourceId> for Types {
    fn index_mut(&mut self, id: ResourceId) -> &mut Self::Output {
        &mut self.resources[id.0]
    }
}

impl IndexMut<FuncTypeId> for Types {
    fn index_mut(&mut self, id: FuncTypeId) -> &mut Self::Output {
        &mut self.funcs[id.0]
    }
}

impl IndexMut<InterfaceId> for Types {
    fn index_mut(&mut self, id: InterfaceId) -> &mut Self::Output {
        &mut self.interfaces[id.0]
    }
}

impl IndexMut<WorldId> for Types {
    fn index_mut(&mut self, id: WorldId) -> &mut Self::Output {
        &mut self.worlds[id.0]
    }
}

impl IndexMut<ModuleTypeId> for Types {
    fn index_mut(&mut self, id: ModuleTypeId) -> &mut Self::Output {
        &mut self.modules[id.0]
    }
}

/// Represents the kind of a component model item.
///
/// Note that an item kind is associated with a particular `Types` collection.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum ItemKind {
    /// The item is a type.
    Type(Type),
    /// The item is a function.
    Func(FuncTypeId),
    /// The item is a component instance.
    Instance(InterfaceId),
    /// The item is a component.
    Component(WorldId),
    /// The item is a core module.
    Module(ModuleTypeId),
    /// The item is a value.
    Value(ValueType),
}

impl ItemKind {
    /// Gets the type of the item.
    ///
    /// Returns `None` for resource items.
    pub fn ty(&self) -> Type {
        match self {
            ItemKind::Type(ty) => *ty,
            ItemKind::Func(id) => Type::Func(*id),
            ItemKind::Instance(id) => Type::Interface(*id),
            ItemKind::Component(id) => Type::World(*id),
            ItemKind::Module(id) => Type::Module(*id),
            ItemKind::Value(ty) => Type::Value(*ty),
        }
    }

    /// Gets a description of the item kind.
    pub fn desc(&self, types: &Types) -> &'static str {
        match self {
            ItemKind::Func(_) => "function",
            ItemKind::Type(ty) => ty.desc(types),
            ItemKind::Instance(_) => "instance",
            ItemKind::Component(_) => "component",
            ItemKind::Module(_) => "module",
            ItemKind::Value(_) => "value",
        }
    }

    /// Promote function types, instance types, and component types
    /// to functions, instances, and components.
    pub fn promote(&self) -> Self {
        match *self {
            ItemKind::Type(Type::Func(id)) => ItemKind::Func(id),
            ItemKind::Type(Type::Interface(id)) => ItemKind::Instance(id),
            ItemKind::Type(Type::World(id)) => ItemKind::Component(id),
            kind => kind,
        }
    }

    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            ItemKind::Type(ty) => ty._visit_defined_types(types, visitor, false),
            ItemKind::Func(id) => types[*id]._visit_defined_types(types, visitor),
            ItemKind::Instance(id) => types[*id]._visit_defined_types(types, visitor),
            ItemKind::Component(id) => types[*id]._visit_defined_types(types, visitor),
            ItemKind::Module(_) => Ok(()),
            ItemKind::Value(ty) => ty._visit_defined_types(types, visitor, false),
        }
    }
}

impl From<ItemKind> for wasm_encoder::ComponentExportKind {
    fn from(value: ItemKind) -> Self {
        match value {
            ItemKind::Type(_) => Self::Type,
            ItemKind::Func(_) => Self::Func,
            ItemKind::Instance(_) => Self::Instance,
            ItemKind::Component(_) => Self::Component,
            ItemKind::Module(_) => Self::Module,
            ItemKind::Value(_) => Self::Value,
        }
    }
}

/// Represent a component model type.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Type {
    /// The type is a resource.
    Resource(ResourceId),
    /// The type is a function type.
    Func(FuncTypeId),
    /// The type is a value type.
    Value(ValueType),
    /// The type is an interface (i.e. instance type).
    Interface(InterfaceId),
    /// The type is a world (i.e. component type).
    World(WorldId),
    /// The type is a core module type.
    Module(ModuleTypeId),
}

impl Type {
    /// Gets a description of the type.
    pub fn desc(&self, types: &Types) -> &'static str {
        match self {
            Type::Resource(_) => "resource",
            Type::Func(_) => "function type",
            Type::Value(ty) => ty.desc(types),
            Type::Interface(_) => "interface",
            Type::World(_) => "world",
            Type::Module(_) => "module type",
        }
    }

    /// Visits each defined type referenced by this type.
    ///
    /// If the visitor returns `Err`, the visiting stops and the error is returned.
    pub fn visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        self._visit_defined_types(types, visitor, true)
    }

    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
        recurse: bool,
    ) -> Result<(), E> {
        match self {
            Type::Module(_) | Type::Resource(_) => Ok(()),
            Type::Func(id) => types[*id]._visit_defined_types(types, visitor),
            Type::Value(ty) => ty._visit_defined_types(types, visitor, recurse),
            Type::Interface(id) => types[*id]._visit_defined_types(types, visitor),
            Type::World(id) => types[*id]._visit_defined_types(types, visitor),
        }
    }
}

/// Represents a primitive type.
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum PrimitiveType {
    /// A `u8` type.
    U8,
    /// A `s8` type.
    S8,
    /// A `u16` type.
    U16,
    /// A `s16` type.
    S16,
    /// A `u32` type.
    U32,
    /// A `s32` type.
    S32,
    /// A `u64` type.
    U64,
    /// A `s64` type.
    S64,
    /// A `f32` type.
    F32,
    /// A `f64` type.
    F64,
    /// A `char` type.
    Char,
    /// A `bool` type.
    Bool,
    /// A `string` type.
    String,
    /// An `error-context` type.
    ErrorContext,
}

impl PrimitiveType {
    /// Gets a description of the primitive type.
    pub fn desc(&self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::S8 => "s8",
            Self::U16 => "u16",
            Self::S16 => "s16",
            Self::U32 => "u32",
            Self::S32 => "s32",
            Self::U64 => "u64",
            Self::S64 => "s64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Char => "char",
            Self::Bool => "bool",
            Self::String => "string",
            Self::ErrorContext => "error-context",
        }
    }
}

impl From<wasmparser::PrimitiveValType> for PrimitiveType {
    fn from(value: wasmparser::PrimitiveValType) -> Self {
        match value {
            wasmparser::PrimitiveValType::Bool => Self::Bool,
            wasmparser::PrimitiveValType::S8 => Self::S8,
            wasmparser::PrimitiveValType::U8 => Self::U8,
            wasmparser::PrimitiveValType::S16 => Self::S16,
            wasmparser::PrimitiveValType::U16 => Self::U16,
            wasmparser::PrimitiveValType::S32 => Self::S32,
            wasmparser::PrimitiveValType::U32 => Self::U32,
            wasmparser::PrimitiveValType::S64 => Self::S64,
            wasmparser::PrimitiveValType::U64 => Self::U64,
            wasmparser::PrimitiveValType::F32 => Self::F32,
            wasmparser::PrimitiveValType::F64 => Self::F64,
            wasmparser::PrimitiveValType::Char => Self::Char,
            wasmparser::PrimitiveValType::String => Self::String,
            wasmparser::PrimitiveValType::ErrorContext => Self::ErrorContext,
        }
    }
}

impl From<PrimitiveType> for wasm_encoder::PrimitiveValType {
    fn from(value: PrimitiveType) -> Self {
        match value {
            PrimitiveType::U8 => Self::U8,
            PrimitiveType::S8 => Self::S8,
            PrimitiveType::U16 => Self::U16,
            PrimitiveType::S16 => Self::S16,
            PrimitiveType::U32 => Self::U32,
            PrimitiveType::S32 => Self::S32,
            PrimitiveType::U64 => Self::U64,
            PrimitiveType::S64 => Self::S64,
            PrimitiveType::F32 => Self::F32,
            PrimitiveType::F64 => Self::F64,
            PrimitiveType::Char => Self::Char,
            PrimitiveType::Bool => Self::Bool,
            PrimitiveType::String => Self::String,
            PrimitiveType::ErrorContext => Self::ErrorContext,
        }
    }
}

/// Represents a value type.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ValueType {
    /// A primitive value type.
    Primitive(PrimitiveType),
    /// The type is a borrow of a resource type.
    Borrow(ResourceId),
    /// The type is an owned resource type.
    Own(ResourceId),
    /// A defined value type.
    Defined(DefinedTypeId),
}

impl ValueType {
    /// Checks if the type contains a borrow.
    ///
    /// Function results may not return a type containing a borrow.
    pub fn contains_borrow(&self, types: &Types) -> bool {
        match self {
            ValueType::Primitive(_) | ValueType::Own(_) => false,
            ValueType::Borrow(_) => true,
            ValueType::Defined(id) => types[*id].contains_borrow(types),
        }
    }

    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
        recurse: bool,
    ) -> Result<(), E> {
        match self {
            ValueType::Primitive(_) | ValueType::Borrow(_) | ValueType::Own(_) => Ok(()),
            ValueType::Defined(id) => {
                visitor(types, *id)?;
                if recurse {
                    types[*id]._visit_defined_types(types, visitor)?;
                }

                Ok(())
            }
        }
    }

    /// Gets a description of the value type.
    pub fn desc(&self, types: &Types) -> &'static str {
        match self {
            Self::Primitive(ty) => ty.desc(),
            Self::Borrow(_) => "borrow",
            Self::Own(_) => "own",
            Self::Defined(id) => types[*id].desc(types),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ValueType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Primitive(ty) => ty.serialize(serializer),
            Self::Borrow(id) => format!("borrow<{id}>", id = id.0.index()).serialize(serializer),
            Self::Own(id) => format!("own<{id}>", id = id.0.index()).serialize(serializer),
            Self::Defined(id) => id.serialize(serializer),
        }
    }
}

/// Represents a defined value type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum DefinedType {
    /// A tuple type.
    Tuple(Vec<ValueType>),
    /// A list type.
    List(ValueType),
    /// A fixed size array
    FixedSizeList(ValueType, u32),
    /// A map type.
    Map(ValueType, ValueType),
    /// An option type.
    Option(ValueType),
    /// A result type.
    Result {
        /// The result's `ok` type.
        ok: Option<ValueType>,
        /// The result's `err` type.
        err: Option<ValueType>,
    },
    /// The type is a variant type.
    Variant(Variant),
    /// The type is a record type.
    Record(Record),
    /// The type is a flags type.
    Flags(Flags),
    /// The type is an enum.
    Enum(Enum),
    /// The type is an alias to another value type.
    Alias(ValueType),
    /// A stream type.
    Stream(Option<ValueType>),
    /// A futures type.
    Future(Option<ValueType>),
}

impl DefinedType {
    /// Determines if the defined type recursively contains a borrow.
    pub fn contains_borrow(&self, types: &Types) -> bool {
        match self {
            Self::Tuple(tys) => tys.iter().any(|ty| ty.contains_borrow(types)),
            Self::List(ty) | Self::FixedSizeList(ty, _) => ty.contains_borrow(types),
            Self::Map(key, value) => key.contains_borrow(types) || value.contains_borrow(types),
            Self::Option(ty) => ty.contains_borrow(types),
            Self::Result { ok, err } => {
                ok.map(|ty| ty.contains_borrow(types)).unwrap_or(false)
                    || err.map(|ty| ty.contains_borrow(types)).unwrap_or(false)
            }
            Self::Variant(v) => v
                .cases
                .values()
                .any(|ty| ty.map(|ty| ty.contains_borrow(types)).unwrap_or(false)),
            Self::Record(r) => r.fields.iter().any(|(_, ty)| ty.contains_borrow(types)),
            Self::Flags(_) => false,
            Self::Enum(_) => false,
            Self::Alias(ty) => ty.contains_borrow(types),
            Self::Stream(ty) => ty.map(|ty| ty.contains_borrow(types)).unwrap_or(false),
            Self::Future(ty) => ty.map(|ty| ty.contains_borrow(types)).unwrap_or(false),
        }
    }

    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            DefinedType::Tuple(tys) => {
                for ty in tys {
                    ty._visit_defined_types(types, visitor, false)?;
                }

                Ok(())
            }
            DefinedType::List(ty) | DefinedType::Option(ty) | DefinedType::FixedSizeList(ty, _) => {
                ty._visit_defined_types(types, visitor, false)
            }
            DefinedType::Map(key, value) => {
                key._visit_defined_types(types, visitor, false)?;
                value._visit_defined_types(types, visitor, false)
            }
            DefinedType::Result { ok, err } => {
                if let Some(ty) = ok.as_ref() {
                    ty._visit_defined_types(types, visitor, false)?;
                }

                if let Some(ty) = err.as_ref() {
                    ty._visit_defined_types(types, visitor, false)?;
                }

                Ok(())
            }
            DefinedType::Variant(v) => {
                for ty in v.cases.values().filter_map(Option::as_ref) {
                    ty._visit_defined_types(types, visitor, false)?;
                }

                Ok(())
            }
            DefinedType::Record(r) => {
                for (_, ty) in &r.fields {
                    ty._visit_defined_types(types, visitor, false)?
                }

                Ok(())
            }
            DefinedType::Flags(_) | DefinedType::Enum(_) | DefinedType::Alias(_) => Ok(()),
            DefinedType::Stream(ty) | DefinedType::Future(ty) => {
                if let Some(ty) = ty {
                    ty._visit_defined_types(types, visitor, false)?;
                }

                Ok(())
            }
        }
    }

    /// Gets atyd | DefinedType::Future(t)escription of the defined type.
    pub fn desc(&self, types: &Types) -> &'static str {
        match self {
            Self::Tuple(_) => "tuple",
            Self::List(_) => "list",
            Self::FixedSizeList(_, _) => "list<,N>",
            Self::Map(_, _) => "map",
            Self::Option(_) => "option",
            Self::Result { .. } => "result",
            Self::Variant(_) => "variant",
            Self::Record(_) => "record",
            Self::Flags(_) => "flags",
            Self::Enum(_) => "enum",
            Self::Alias(ty) => ty.desc(types),
            Self::Stream(_) => "stream",
            Self::Future(_) => "future",
        }
    }
}

/// Represents a kind of function in the component model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum FuncKind {
    /// The function is a "free" function (i.e. not associated with a resource).
    Free,
    /// The function is a method on a resource.
    Method,
    /// The function is a static method on a resource.
    Static,
    /// The function is a resource constructor.
    Constructor,
}

impl fmt::Display for FuncKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FuncKind::Free => write!(f, "function"),
            FuncKind::Method => write!(f, "method"),
            FuncKind::Static => write!(f, "static method"),
            FuncKind::Constructor => write!(f, "constructor"),
        }
    }
}

/// Represents information about an aliased resource.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ResourceAlias {
    /// The foreign owning interface for the resource.
    ///
    /// This may be `None` if the resource does not have a foreign interface owner
    /// such as in a world or when aliasing within the same interface.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub owner: Option<InterfaceId>,
    /// The id of the resource that was aliased.
    pub source: ResourceId,
}

/// Represents a resource type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Resource {
    /// The name of the resource.
    pub name: String,
    /// Information if the resource has been aliased.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub alias: Option<ResourceAlias>,
}

/// Represents a variant.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Variant {
    /// The variant cases.
    pub cases: IndexMap<String, Option<ValueType>>,
}

/// Represents a record type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Record {
    /// The record fields.
    pub fields: IndexMap<String, ValueType>,
}

/// Represents a flags type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Flags(pub IndexSet<String>);

/// Represents an enum type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Enum(pub IndexSet<String>);

/// Represents a function type.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct FuncType {
    /// The parameters of the function.
    pub params: IndexMap<String, ValueType>,
    /// The result of the function.
    pub result: Option<ValueType>,
    /// Whether or not this is an async function.
    pub is_async: bool,
}

impl FuncType {
    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        for ty in self.params.values() {
            ty._visit_defined_types(types, visitor, false)?;
        }

        if let Some(ty) = &self.result {
            ty._visit_defined_types(types, visitor, false)?;
        }

        Ok(())
    }
}

/// Represents a used type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct UsedType {
    /// The interface the type was used from.
    pub interface: InterfaceId,
    /// The original export name.
    ///
    /// This is `None` when the type was not renamed with an `as` clause.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub name: Option<String>,
}

/// Represents an interface (i.e. instance type).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Interface {
    /// The identifier of the interface.
    ///
    /// This may be `None` for inline interfaces.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub id: Option<String>,
    /// A map of exported name to information about the used type.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub uses: IndexMap<String, UsedType>,
    /// The exported items of the interface.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub exports: IndexMap<String, ItemKind>,
}

impl Interface {
    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        for kind in self.exports.values() {
            kind._visit_defined_types(types, visitor)?;
        }

        Ok(())
    }
}

/// Represents a world.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct World {
    /// The identifier of the world.
    ///
    /// This may be `None` for worlds representing component types.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub id: Option<String>,
    /// A map of imported name to information about the used type.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub uses: IndexMap<String, UsedType>,
    /// The imported items of the world.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub imports: IndexMap<String, ItemKind>,
    /// The exported items of the world.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub exports: IndexMap<String, ItemKind>,
}

impl World {
    fn _visit_defined_types<'a, E>(
        &self,
        types: &'a Types,
        visitor: &mut impl FnMut(&'a Types, DefinedTypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        for kind in self.imports.values() {
            kind._visit_defined_types(types, visitor)?;
        }

        for kind in self.exports.values() {
            kind._visit_defined_types(types, visitor)?;
        }

        Ok(())
    }

    /// The interfaces imported implicitly through uses.
    pub fn implicit_imported_interfaces<'a>(
        &'a self,
        types: &'a Types,
    ) -> IndexMap<&'a str, ItemKind> {
        let mut interfaces = IndexMap::new();
        let mut add_interface_for_used_type = |used_item: &UsedType| {
            let used_interface_id = used_item.interface;
            // The id must be set since used interfaces are always named.
            let used_interface_name = types[used_interface_id].id.as_deref().unwrap();
            interfaces.insert(used_interface_name, ItemKind::Instance(used_interface_id));
        };

        for (_, used_type) in self.uses.iter() {
            add_interface_for_used_type(used_type);
        }

        for (_, import) in self.imports.iter() {
            if let ItemKind::Instance(interface_id) = import {
                let import = &types[*interface_id];
                for (_, used_item) in &import.uses {
                    add_interface_for_used_type(used_item);
                }
            }
        }
        interfaces
    }
}

/// Represents a kind of an extern item.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ExternKind {
    /// The item is an import.
    Import,
    /// The item is an export.
    Export,
}

impl fmt::Display for ExternKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Import => write!(f, "import"),
            Self::Export => write!(f, "export"),
        }
    }
}
