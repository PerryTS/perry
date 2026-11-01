// Modified by Perry: preserve imported resource identities in root function types.
// See ../VENDORED.md for the pinned upstream source and patch description.
use crate::PackageId;
use indexmap::IndexMap;
use petgraph::graph::NodeIndex;
use std::borrow::Cow;
use std::collections::HashMap;
use wac_types::{
    CoreExtern, DefinedType, DefinedTypeId, Enum, Flags, FuncTypeId, InterfaceId, ItemKind,
    ModuleTypeId, PrimitiveType, Record, ResourceId, Type, Types, UsedType, ValueType, Variant,
    WorldId,
};
use wasm_encoder::{
    Alias, ComponentBuilder, ComponentCoreTypeEncoder, ComponentExportKind, ComponentExternName,
    ComponentOuterAliasKind, ComponentType, ComponentTypeEncoder, ComponentTypeRef,
    ComponentValType, EntityType, GlobalType, InstanceType, MemoryType, ModuleType, TableType,
    TagKind, TagType, TypeBounds,
};
use wasmparser::names::{ComponentName, ComponentNameKind};

/// Determines whether an instance import/export needs a component model
/// `(implements "…")` directive, and if so returns the interface name to put
/// inside it.
///
/// An instance extern is encoded one of two ways depending on its name:
///
/// * Regular interface extern: the extern name *is* the interface name, e.g.
///   `(import "foo:bar/iface" (instance …))`. Nothing to disambiguate, so this
///   returns `None`.
///
/// * `implements` extern: the extern name is a plain-name label and the
///   interface it implements is carried separately, e.g.
///   `(import "primary" (implements "foo:bar/iface") (instance …))`. This is
///   how a component imports/exports the same interface more than once under
///   different labels. Here this returns `Some("foo:bar/iface")`, i.e. the
///   string the caller should encode as the `implements` directive.
///
/// `name` is the extern name (`primary`) and `interface` identifies the
/// instance's interface within `types`, whose own name (`foo:bar/iface`) is the
/// returned value.
pub(crate) fn implements_directive<'a>(
    types: &'a Types,
    name: &str,
    interface: InterfaceId,
) -> Option<&'a str> {
    // Only a plain-name label is encoded with an `implements` directive; a
    // regular interface extern already names its interface directly.
    let is_label = matches!(
        ComponentName::new(name, 0).as_ref().map(|n| n.kind()),
        Ok(ComponentNameKind::Label(_))
    );
    if !is_label {
        return None;
    }
    types[interface].id.as_deref()
}

/// A type used to abstract the API differences between a component builder,
/// component type, and instance type from `wasm-encoder`.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum Encodable {
    Builder(ComponentBuilder),
    Instance(InstanceType),
    Component(ComponentType),
}

impl Encodable {
    fn type_count(&self) -> u32 {
        match self {
            Encodable::Builder(t) => t.type_count(),
            Encodable::Component(t) => t.type_count(),
            Encodable::Instance(t) => t.type_count(),
        }
    }

    fn instance_count(&self) -> u32 {
        match self {
            Encodable::Builder(t) => t.instance_count(),
            Encodable::Component(t) => t.instance_count(),
            Encodable::Instance(t) => t.instance_count(),
        }
    }

    fn core_type_count(&self) -> u32 {
        match self {
            Encodable::Builder(t) => t.core_type_count(),
            Encodable::Component(t) => t.core_type_count(),
            Encodable::Instance(t) => t.core_type_count(),
        }
    }

    fn ty(&mut self) -> ComponentTypeEncoder<'_> {
        match self {
            Encodable::Builder(t) => t.ty(None).1,
            Encodable::Instance(t) => t.ty(),
            Encodable::Component(t) => t.ty(),
        }
    }

    fn core_type(&mut self) -> ComponentCoreTypeEncoder<'_> {
        match self {
            Encodable::Builder(t) => t.core_type(None).1,
            Encodable::Instance(t) => t.core_type(),
            Encodable::Component(t) => t.core_type(),
        }
    }

    fn import_type<'a>(&mut self, name: impl Into<ComponentExternName<'a>>, ty: ComponentTypeRef) {
        let name = name.into();
        match self {
            Encodable::Component(t) => {
                t.import(name, ty);
            }
            Encodable::Builder(b) => {
                b.import(name, ty);
            }
            _ => panic!("expected a component type"),
        }
    }

    fn alias(&mut self, alias: Alias) {
        match self {
            Encodable::Builder(t) => {
                t.alias(None, alias);
            }
            Encodable::Instance(t) => {
                t.alias(alias);
            }
            Encodable::Component(t) => {
                t.alias(alias);
            }
        }
    }
}

impl Default for Encodable {
    fn default() -> Self {
        Self::Builder(Default::default())
    }
}

#[derive(Debug, Default)]
pub struct Scope {
    /// The map from types to encoded type index.
    pub type_indexes: IndexMap<Type, u32>,
    /// Concrete interface identities, including separately labeled instances
    /// implementing the same canonical name. Names are not nominal identities.
    pub instances: IndexMap<InterfaceId, u32>,
    /// Resource identities exported by imported instances, including anonymous ones.
    resource_instances: IndexMap<ResourceId, (u32, String)>,
    /// The map of import/export name to their alias indexes.
    type_aliases: IndexMap<String, u32>,
    /// The map of resource names to their encoded indexes.
    resources: IndexMap<String, u32>,
    /// The encodable for this scope.
    encodable: Encodable,
}

#[derive(Debug, Default)]
pub struct State {
    /// The stack of encoding scopes.
    scopes: Vec<Scope>,
    /// The current encoding scope.
    pub current: Scope,
    /// A map of nodes in the graph to their encoded indexes.
    pub node_indexes: HashMap<NodeIndex, u32>,
    /// The map of package identifiers to encoded components (either imported or defined).
    pub packages: HashMap<PackageId, u32>,
    /// A map of instantiation nodes to a list of their encoded implicitly imported arguments.
    pub implicit_args: HashMap<NodeIndex, Vec<(String, ComponentExportKind, u32)>>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builder(&mut self) -> &mut ComponentBuilder {
        assert!(self.scopes.is_empty(), "expected scopes to be empty");
        match &mut self.current.encodable {
            Encodable::Builder(builder) => builder,
            _ => panic!("expected a builder"),
        }
    }

    fn push(&mut self, encodable: Encodable) {
        log::debug!("pushing new type scope");
        let prev = std::mem::replace(
            &mut self.current,
            Scope {
                encodable,
                ..Default::default()
            },
        );

        self.scopes.push(prev);
    }

    fn pop(&mut self) -> Encodable {
        log::debug!("popping type scope");
        let prev = std::mem::replace(
            &mut self.current,
            self.scopes.pop().expect("expected a scope to pop"),
        );
        prev.encodable
    }

    fn used_type_index(&mut self, name: &str) -> Option<u32> {
        if let Some(index) = self.current.type_aliases.get(name) {
            return Some(*index);
        }

        if let Some(parent) = self.scopes.last() {
            if let Some(outer) = parent.type_aliases.get(name) {
                let index = self.current.encodable.type_count();
                log::debug!("encoding outer alias for `{name}` to type index {index}");
                self.current.encodable.alias(Alias::Outer {
                    kind: ComponentOuterAliasKind::Type,
                    count: 1,
                    index: *outer,
                });
                return Some(index);
            }
        }

        None
    }
}

pub struct TypeEncoder<'a>(&'a Types);

impl<'a> TypeEncoder<'a> {
    pub fn new(types: &'a Types) -> Self {
        Self(types)
    }

    // Aliases a value type from an imported instance that exports it, rather
    // than re-encoding it locally: a type import must reference a named type,
    // not a local definition.
    fn alias_from_instance_export(&self, state: &mut State, ty: ValueType) -> Option<u32> {
        if state.current.instances.is_empty() {
            return None;
        }
        let ty = Type::Value(ty);
        let (instance, name) = state.current.instances.iter().find_map(|(id, instance)| {
            self.0[*id].exports.iter().find_map(|(name, kind)| {
                (*kind == ItemKind::Type(ty)).then(|| (*instance, name.clone()))
            })
        })?;
        let index = state.current.encodable.type_count();
        state.current.encodable.alias(Alias::InstanceExport {
            instance,
            kind: ComponentExportKind::Type,
            name: &name,
        });
        state.current.type_indexes.insert(ty, index);
        Some(index)
    }

    pub fn ty(&self, state: &mut State, ty: Type, name: Option<&str>) -> u32 {
        if let Some(index) = state.current.type_indexes.get(&ty) {
            return *index;
        }

        if let Some(name) = name {
            if let Some(index) = state.used_type_index(name) {
                state.current.type_indexes.insert(ty, index);
                return index;
            }
        }

        let index = match ty {
            Type::Resource(_) => panic!("cannot encode a resource"),
            Type::Func(id) => self.func_type(state, id),
            Type::Value(ValueType::Primitive(ty)) => Self::primitive(state, ty),
            Type::Value(ValueType::Borrow(id)) => self.borrow(state, id),
            Type::Value(ValueType::Own(id)) => self.own(state, id),
            Type::Value(ValueType::Defined(id)) => self.defined(state, id),
            Type::Interface(id) => self.instance(state, id, false),
            Type::World(id) => self.component(state, id),
            Type::Module(id) => self.module(state, id),
        };

        state.current.type_indexes.insert(ty, index);
        index
    }

    fn func_type(&self, state: &mut State, id: FuncTypeId) -> u32 {
        log::debug!("encoding function type");
        let ty = &self.0[id];

        let params = ty
            .params
            .iter()
            .map(|(n, ty)| (n.as_str(), self.value_type(state, *ty)))
            .collect::<Vec<_>>();

        let result = ty.result.map(|ty| self.value_type(state, ty));
        let index = state.current.encodable.type_count();
        let mut encoder = state.current.encodable.ty().function();
        encoder.async_(ty.is_async);
        encoder.params(params);
        encoder.result(result);

        log::debug!("function type encoded to type index {index}");
        index
    }

    fn defined(&self, state: &mut State, id: DefinedTypeId) -> u32 {
        log::debug!("encoding defined type");
        let ty = &self.0[id];
        let index = match ty {
            DefinedType::Tuple(types) => self.tuple(state, types),
            DefinedType::List(ty) => self.list(state, *ty),
            DefinedType::FixedSizeList(ty, elements) => self.fixed_size_list(state, *ty, *elements),
            DefinedType::Map(key, value) => self.map(state, *key, *value),
            DefinedType::Option(ty) => self.option(state, *ty),
            DefinedType::Result { ok, err } => self.result(state, *ok, *err),
            DefinedType::Variant(v) => self.variant(state, v),
            DefinedType::Record(r) => self.record(state, r),
            DefinedType::Flags(f) => self.flags(state, f),
            DefinedType::Enum(e) => self.enum_type(state, e),
            DefinedType::Alias(ValueType::Primitive(ty)) => Self::primitive(state, *ty),
            DefinedType::Alias(ValueType::Borrow(id)) => self.borrow(state, *id),
            DefinedType::Alias(ValueType::Own(id)) => self.own(state, *id),
            DefinedType::Alias(ValueType::Defined(id)) => self.defined(state, *id),
            DefinedType::Stream(ty) => self.stream(state, *ty),
            DefinedType::Future(ty) => self.future(state, *ty),
        };

        log::debug!("defined type encoded to type index {index}");
        index
    }

    fn use_aliases(
        &self,
        state: &mut State,
        uses: &'a IndexMap<String, UsedType>,
        items: &'a IndexMap<String, ItemKind>,
    ) {
        state.current.type_aliases.clear();

        for (name, used) in uses {
            let interface = &self.0[used.interface];
            let iid = interface.id.as_ref().expect("interface should have an id");
            let instance = state.current.instances[&used.interface];
            let index = state.current.encodable.type_count();
            let export: &String = used.name.as_ref().unwrap_or(name);
            let kind = interface.exports.get(export).unwrap();
            state.current.encodable.alias(Alias::InstanceExport {
                instance,
                kind: ComponentExportKind::Type,
                name: export,
            });

            log::debug!(
                "aliased {kind} export `{export}` of instance index {instance} ({iid}) to type index {index}",
                kind = kind.desc(self.0)
            );

            state.current.type_aliases.insert(name.clone(), index);

            // Record the alias under both ids that denote this used type (the
            // interface's export and the using entity's own item) so a later
            // id-keyed reference reuses it instead of re-encoding a local copy:
            // a type import must reference a named type, not a local definition.
            if let ItemKind::Type(ty) = kind {
                state.current.type_indexes.insert(*ty, index);
            }
            if let Some(ItemKind::Type(ty)) = items.get(name) {
                state.current.type_indexes.insert(*ty, index);
            }
        }
    }

    fn instance(&self, state: &mut State, id: InterfaceId, types_only: bool) -> u32 {
        log::debug!("encoding instance type");
        let interface = &self.0[id];
        for used in interface.uses.values() {
            self.import_deps(state, used.interface);
        }

        // Encode any required aliases
        self.use_aliases(state, &interface.uses, &interface.exports);
        state.push(Encodable::Instance(InstanceType::default()));

        // Otherwise, export all exports
        for (name, kind) in &interface.exports {
            match kind {
                ItemKind::Type(_) => {
                    self.export(state, name, *kind);
                }
                _ => {
                    if !types_only {
                        self.export(state, name, *kind);
                    }
                }
            }
        }

        match state.pop() {
            Encodable::Instance(ty) => {
                let index = state.current.encodable.type_count();
                state.current.encodable.ty().instance(&ty);
                log::debug!("instance type encoded to type index {index}");
                index
            }
            _ => panic!("expected the pushed encodable to be an instance type"),
        }
    }

    pub fn component(&self, state: &mut State, id: WorldId) -> u32 {
        log::debug!("encoding component type");
        let world = &self.0[id];

        state.push(Encodable::Component(ComponentType::default()));

        for used in world.uses.values() {
            self.import_deps(state, used.interface);
        }

        self.use_aliases(state, &world.uses, &world.imports);

        for (name, kind) in &world.imports {
            self.import(state, name, *kind);
        }

        for (name, kind) in &world.exports {
            self.export(state, name, *kind);
        }

        match state.pop() {
            Encodable::Component(ty) => {
                let index = state.current.encodable.type_count();
                state.current.encodable.ty().component(&ty);
                log::debug!("component type encoded to type index {index}");
                index
            }
            _ => panic!("expected the pushed encodable to be a component type"),
        }
    }

    fn import_deps(&self, state: &mut State, id: InterfaceId) {
        let iid = self.0[id].id.as_ref().expect("interface should have an id");
        if state.current.instances.contains_key(&id) {
            return;
        }

        let interface = &self.0[id];

        // Depth-first recurse on the dependencies of this interface
        for used in interface.uses.values() {
            self.import_deps(state, used.interface);
        }

        log::debug!("encoding dependency on interface `{iid}`");

        let index = self.instance(state, id, !state.scopes.is_empty());
        let import_index = state.current.encodable.instance_count();

        state
            .current
            .encodable
            .import_type(iid, ComponentTypeRef::Instance(index));

        log::debug!(
            "interface `{iid}` is available for aliasing from instance index {import_index}"
        );
        state.current.instances.insert(id, import_index);
        self.record_instance_resources(state, id, import_index);
    }

    pub fn interface(&self, state: &mut State, id: InterfaceId) -> u32 {
        let interface = &self.0[id];
        let iid = interface.id.as_deref().expect("interface must have an id");
        log::debug!("encoding interface definition of `{iid}`");
        assert!(state.scopes.is_empty());
        state.push(Encodable::Component(ComponentType::default()));

        for used in interface.uses.values() {
            self.import_deps(state, used.interface);
        }

        let index = self.instance(state, id, false);
        Self::export_type(state, iid, ComponentTypeRef::Instance(index));

        match state.pop() {
            Encodable::Component(ty) => {
                let (index, encoder) = state.builder().ty(None);
                encoder.component(&ty);
                log::debug!("encoded interface definition of `{iid}` to type index {index}",);
                index
            }
            _ => panic!("expected the pushed encodable to be a component type"),
        }
    }

    pub fn world(&self, state: &mut State, id: WorldId) -> u32 {
        let world = &self.0[id];
        let world_id = world.id.as_deref().expect("world must have an id");
        log::debug!("encoding world definition of `{world_id}`");

        assert!(state.scopes.is_empty());
        state.push(Encodable::Component(ComponentType::default()));
        let index = self.component(state, id);
        Self::export_type(state, world_id, ComponentTypeRef::Component(index));

        match state.pop() {
            Encodable::Component(ty) => {
                let (index, encoder) = state.builder().ty(None);
                encoder.component(&ty);
                log::debug!("encoded world definition of `{world_id}` to type index {index}");
                index
            }
            _ => panic!("expected the push encodable to be a component type"),
        }
    }

    fn module(&self, state: &mut State, id: ModuleTypeId) -> u32 {
        log::debug!("encoding module definition");
        let ty = &self.0[id];
        let mut encoded = ModuleType::new();

        for ((module, name), ext) in &ty.imports {
            let ty = self.entity_type(&mut encoded, ext);
            encoded.import(module, name, ty);
        }

        for (name, ext) in &ty.exports {
            let ty = self.entity_type(&mut encoded, ext);
            encoded.export(name, ty);
        }

        let index = state.current.encodable.core_type_count();
        state.current.encodable.core_type().module(&encoded);
        log::debug!("encoded module definition to type index {index}");
        index
    }

    fn entity_type(&self, encodable: &mut ModuleType, ext: &CoreExtern) -> EntityType {
        match ext {
            CoreExtern::Func(func) => {
                let index = encodable.type_count();
                encodable.ty().function(
                    func.params.iter().copied().map(Into::into),
                    func.results.iter().copied().map(Into::into),
                );
                EntityType::Function(index)
            }
            CoreExtern::Table {
                element_type,
                initial,
                maximum,
                table64,
                shared,
            } => EntityType::Table(TableType {
                element_type: (*element_type).into(),
                minimum: *initial,
                maximum: *maximum,
                table64: *table64,
                shared: *shared,
            }),
            CoreExtern::Memory {
                memory64,
                shared,
                initial,
                maximum,
                page_size_log2,
            } => EntityType::Memory(MemoryType {
                minimum: *initial,
                maximum: *maximum,
                memory64: *memory64,
                shared: *shared,
                page_size_log2: *page_size_log2,
            }),
            CoreExtern::Global {
                val_type,
                mutable,
                shared,
            } => EntityType::Global(GlobalType {
                val_type: (*val_type).into(),
                mutable: *mutable,
                shared: *shared,
            }),
            CoreExtern::Tag(func) => {
                let index = encodable.type_count();
                encodable.ty().function(
                    func.params.iter().copied().map(Into::into),
                    func.results.iter().copied().map(Into::into),
                );
                EntityType::Tag(TagType {
                    kind: TagKind::Exception,
                    func_type_idx: index,
                })
            }
        }
    }

    pub fn value_type(&self, state: &mut State, ty: ValueType) -> ComponentValType {
        if let Some(index) = state.current.type_indexes.get(&Type::Value(ty)) {
            return ComponentValType::Type(*index);
        }

        if let Some(index) = self.alias_from_instance_export(state, ty) {
            return ComponentValType::Type(index);
        }

        let index = match ty {
            ValueType::Primitive(ty) => return ComponentValType::Primitive(ty.into()),
            ValueType::Borrow(id) => self.borrow(state, id),
            ValueType::Own(id) => self.own(state, id),
            ValueType::Defined(id) => self.defined(state, id),
        };

        state.current.type_indexes.insert(Type::Value(ty), index);
        ComponentValType::Type(index)
    }

    fn primitive(state: &mut State, ty: PrimitiveType) -> u32 {
        let index = state.current.encodable.type_count();
        state
            .current
            .encodable
            .ty()
            .defined_type()
            .primitive(ty.into());
        index
    }

    fn tuple(&self, state: &mut State, types: &[ValueType]) -> u32 {
        let types = types
            .iter()
            .map(|ty| self.value_type(state, *ty))
            .collect::<Vec<_>>();
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().tuple(types);
        index
    }

    fn list(&self, state: &mut State, ty: ValueType) -> u32 {
        let ty = self.value_type(state, ty);
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().list(ty);
        index
    }

    fn map(&self, state: &mut State, key: ValueType, value: ValueType) -> u32 {
        let key = self.value_type(state, key);
        let value = self.value_type(state, value);
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().map(key, value);
        index
    }

    fn fixed_size_list(&self, state: &mut State, ty: ValueType, elements: u32) -> u32 {
        let ty = self.value_type(state, ty);
        let index = state.current.encodable.type_count();
        state
            .current
            .encodable
            .ty()
            .defined_type()
            .fixed_length_list(ty, elements);
        index
    }

    fn stream(&self, state: &mut State, ty: Option<ValueType>) -> u32 {
        let ty = ty.map(|ty| self.value_type(state, ty));
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().stream(ty);
        index
    }

    fn future(&self, state: &mut State, ty: Option<ValueType>) -> u32 {
        let ty = ty.map(|ty| self.value_type(state, ty));
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().future(ty);
        index
    }

    fn option(&self, state: &mut State, ty: ValueType) -> u32 {
        let ty = self.value_type(state, ty);
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().option(ty);
        index
    }

    fn result(&self, state: &mut State, ok: Option<ValueType>, err: Option<ValueType>) -> u32 {
        let ok = ok.map(|ty| self.value_type(state, ty));
        let err = err.map(|ty| self.value_type(state, ty));
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().result(ok, err);
        index
    }

    // Function types at the component root can use resources exported by an
    // imported instance. Preserve the original nominal identity by aliasing its
    // type export; the scoped resource-name table does not contain this alias.
    fn resource_index(&self, state: &mut State, res: ResourceId) -> u32 {
        if !state.scopes.is_empty() {
            if let Some(index) = state.current.resources.get(self.0[res].name.as_str()) {
                return *index;
            }
            if let Some(index) = state.used_type_index(self.0[res].name.as_str()) {
                return index;
            }
        }
        let source = self.0.resolve_resource(res);
        for id in [res, source] {
            if let Some(index) = state.current.type_indexes.get(&Type::Resource(id)) {
                return *index;
            }
        }
        let imported = [res, source]
            .into_iter()
            .find_map(|id| state.current.resource_instances.get(&id).cloned());
        let index = state.current.encodable.type_count();
        if let Some((instance, name)) = imported {
            state.current.encodable.alias(Alias::InstanceExport {
                instance,
                kind: ComponentExportKind::Type,
                name: &name,
            });
        } else {
            // An anonymous imported instance can use an outer resource in a
            // function without re-exporting it. Materialize that resource in
            // the owning outer scope before aliasing it into this type.
            let (count, outer_index) = state
                .scopes
                .iter_mut()
                .rev()
                .enumerate()
                .find_map(|(depth, outer)| {
                    let existing = [res, source]
                        .into_iter()
                        .find_map(|id| outer.type_indexes.get(&Type::Resource(id)).copied());
                    let index = if let Some(index) = existing {
                        index
                    } else {
                        let (instance, name) = [res, source]
                            .into_iter()
                            .find_map(|id| outer.resource_instances.get(&id).cloned())?;
                        let index = outer.encodable.type_count();
                        outer.encodable.alias(Alias::InstanceExport {
                            instance,
                            kind: ComponentExportKind::Type,
                            name: &name,
                        });
                        outer.type_indexes.insert(Type::Resource(res), index);
                        outer.type_indexes.insert(Type::Resource(source), index);
                        index
                    };
                    Some((u32::try_from(depth + 1).unwrap(), index))
                })
                .expect("resource needs an imported instance export or outer type");
            state.current.encodable.alias(Alias::Outer {
                kind: ComponentOuterAliasKind::Type,
                count,
                index: outer_index,
            });
        }
        state
            .current
            .type_indexes
            .insert(Type::Resource(res), index);
        state
            .current
            .type_indexes
            .insert(Type::Resource(source), index);
        index
    }
    pub fn record_instance_resources(&self, state: &mut State, id: InterfaceId, instance: u32) {
        for (name, kind) in &self.0[id].exports {
            if let ItemKind::Type(Type::Resource(resource)) = kind {
                let source = self.0.resolve_resource(*resource);
                for id in [*resource, source] {
                    state
                        .current
                        .resource_instances
                        .entry(id)
                        .or_insert_with(|| (instance, name.clone()));
                }
            }
        }
    }
    fn borrow(&self, state: &mut State, res: ResourceId) -> u32 {
        let res = self.resource_index(state, res);
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().borrow(res);
        index
    }

    fn own(&self, state: &mut State, res: ResourceId) -> u32 {
        let res = self.resource_index(state, res);
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().own(res);
        index
    }

    fn variant(&self, state: &mut State, variant: &Variant) -> u32 {
        let cases = variant
            .cases
            .iter()
            .map(|(n, ty)| (n.as_str(), ty.map(|ty| self.value_type(state, ty))))
            .collect::<Vec<_>>();

        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().variant(cases);
        index
    }

    fn record(&self, state: &mut State, record: &Record) -> u32 {
        let fields = record
            .fields
            .iter()
            .map(|(n, ty)| (n.as_str(), self.value_type(state, *ty)))
            .collect::<Vec<_>>();
        let index = state.current.encodable.type_count();
        state.current.encodable.ty().defined_type().record(fields);
        index
    }

    fn flags(&self, state: &mut State, flags: &Flags) -> u32 {
        let index = state.current.encodable.type_count();
        state
            .current
            .encodable
            .ty()
            .defined_type()
            .flags(flags.0.iter().map(String::as_str));
        index
    }

    fn enum_type(&self, state: &mut State, e: &Enum) -> u32 {
        let index = state.current.encodable.type_count();
        state
            .current
            .encodable
            .ty()
            .defined_type()
            .enum_type(e.0.iter().map(String::as_str));
        index
    }

    fn import(&self, state: &mut State, name: &str, kind: ItemKind) {
        if let ItemKind::Type(Type::Resource(id)) = kind {
            self.import_resource(state, name, id);
            return;
        }

        log::debug!("encoding {kind} import `{name}`", kind = kind.desc(self.0));
        let ty = kind.ty();
        let index = self.ty(state, ty, Some(name));

        match kind {
            ItemKind::Type(_) => {
                let import_index = state.current.encodable.type_count();
                state
                    .current
                    .encodable
                    .import_type(name, ComponentTypeRef::Type(TypeBounds::Eq(index)));

                // Remap the type to the index of the imported item
                state.current.type_indexes.insert(ty, import_index);
            }
            ItemKind::Func(_) => {
                state
                    .current
                    .encodable
                    .import_type(name, ComponentTypeRef::Func(index));
            }
            ItemKind::Instance(id) => {
                let import_index = state.current.encodable.instance_count();
                let implements = implements_directive(self.0, name, id);
                let extern_name = ComponentExternName {
                    name: Cow::Borrowed(name),
                    implements: implements.map(Cow::Borrowed),
                    external_id: None,
                    version_suffix: None,
                };
                state
                    .current
                    .encodable
                    .import_type(extern_name, ComponentTypeRef::Instance(index));
                self.record_instance_resources(state, id, import_index);
                log::debug!("instance index {import_index} ({id:?}) is available for aliasing");
                state.current.instances.insert(id, import_index);
            }
            _ => panic!("expected only types, functions, and instance types"),
        }
    }

    pub fn import_resource(&self, state: &mut State, name: &str, id: ResourceId) -> u32 {
        if let Some(index) = state.current.resources.get(name) {
            return *index;
        }

        log::debug!("encoding import of resource `{name}`");

        let resource = &self.0[id];
        let index = if let Some(outer) = state.used_type_index(name) {
            // This is an alias to an outer resource type
            let index = state.current.encodable.type_count();
            state
                .current
                .encodable
                .import_type(name, ComponentTypeRef::Type(TypeBounds::Eq(outer)));

            log::debug!("encoded outer alias for resource `{name}` to type index {index}");
            index
        } else if let Some(alias) = resource.alias {
            let source = self.0.resolve_resource(alias.source);
            let source_index = if let Some(index) =
                state.current.resources.get(self.0[source].name.as_str())
            {
                // The source resource was previously imported
                *index
            } else if let Some(index) = state.current.type_indexes.get(&Type::Resource(source)) {
                // The source resource isn't directly imported, but was previously aliased
                *index
            } else {
                // Otherwise, we need to alias the source resource
                // This should only occur for resources owned by interfaces
                let source_index = state.current.encodable.type_count();
                let owner = alias.owner.expect("should have owner");
                state.current.encodable.alias(Alias::InstanceExport {
                    instance: state.current.instances[&owner],
                    kind: ComponentExportKind::Type,
                    name: self.0[source].name.as_str(),
                });
                state
                    .current
                    .type_indexes
                    .insert(Type::Resource(source), source_index);
                source_index
            };

            let index = state.current.encodable.type_count();
            state
                .current
                .encodable
                .import_type(name, ComponentTypeRef::Type(TypeBounds::Eq(source_index)));

            log::debug!("encoded import for resource `{name}` as type index {index} (alias of type index {source_index})");
            index
        } else {
            // Otherwise, this is a new resource type, import with a subtype bounds
            let index = state.current.encodable.type_count();
            state
                .current
                .encodable
                .import_type(name, ComponentTypeRef::Type(TypeBounds::SubResource));

            log::debug!("encoded import for resource `{name}` to type index {index}");
            index
        };

        state.current.resources.insert(resource.name.clone(), index);
        state.current.type_indexes.insert(Type::Resource(id), index);
        index
    }

    fn export(&self, state: &mut State, name: &str, kind: ItemKind) -> u32 {
        if let ItemKind::Type(Type::Resource(id)) = kind {
            return self.export_resource(state, name, id);
        }

        log::debug!(
            "encoding {kind} export of `{name}`",
            kind = kind.desc(self.0)
        );

        let ty = kind.ty();
        let index = self.ty(state, ty, Some(name));
        // Preserve the `(implements "I")` directive when re-encoding an
        // instance export that uses a plain-name label.
        let implements = match kind {
            ItemKind::Instance(id) => implements_directive(self.0, name, id),
            _ => None,
        };
        let extern_name = ComponentExternName {
            name: Cow::Borrowed(name),
            implements: implements.map(Cow::Borrowed),
            external_id: None,
            version_suffix: None,
        };
        let index = Self::export_type(
            state,
            extern_name,
            match kind {
                ItemKind::Type(_) => ComponentTypeRef::Type(TypeBounds::Eq(index)),
                ItemKind::Func(_) => ComponentTypeRef::Func(index),
                ItemKind::Instance(_) => ComponentTypeRef::Instance(index),
                _ => panic!("expected only types, functions, and instance types"),
            },
        );

        // For types, remap to the index of the exported item
        if let ItemKind::Type(ty) = kind {
            state.current.type_indexes.insert(ty, index);
        }

        index
    }

    fn export_resource(&self, state: &mut State, name: &str, id: ResourceId) -> u32 {
        log::debug!("encoding export of resource `{name}`");

        if let Some(existing) = state.current.resources.get(name) {
            return *existing;
        }

        let resource = &self.0[id];
        let index = if let Some(outer) = state.used_type_index(name) {
            // This is an alias to an outer resource type
            let index =
                Self::export_type(state, name, ComponentTypeRef::Type(TypeBounds::Eq(outer)));
            log::debug!("encoded outer alias for resource `{name}` as type index {index}");
            index
        } else if let Some(alias) = resource.alias {
            // This is an alias to another resource at the same scope
            let index = state.current.resources
                [self.0[self.0.resolve_resource(alias.source)].name.as_str()];
            let index =
                Self::export_type(state, name, ComponentTypeRef::Type(TypeBounds::Eq(index)));
            log::debug!("encoded alias for resource `{name}` as type index {index}");
            index
        } else {
            // Otherwise, this is a new resource type, export with a subtype bounds
            let index =
                Self::export_type(state, name, ComponentTypeRef::Type(TypeBounds::SubResource));
            log::debug!("encoded export of resource `{name}` as type index {index}");
            index
        };

        state.current.resources.insert(resource.name.clone(), index);
        index
    }

    fn export_type<'b>(
        state: &mut State,
        name: impl Into<ComponentExternName<'b>>,
        ty: ComponentTypeRef,
    ) -> u32 {
        let name = name.into();
        match &mut state.current.encodable {
            Encodable::Component(t) => {
                let index = t.type_count();
                t.export(name, ty);
                index
            }
            Encodable::Instance(t) => {
                let index = t.type_count();
                t.export(name, ty);
                index
            }
            Encodable::Builder(_) => panic!("expected a component or instance type"),
        }
    }
}
