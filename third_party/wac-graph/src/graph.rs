// Modified by Perry: record nominal resource exports of root instance imports.
// See ../VENDORED.md for the pinned upstream source and patch description.
use crate::encoding::{State, TypeEncoder};
use indexmap::IndexMap;
use petgraph::{
    dot::{Config, Dot},
    graph::NodeIndex,
    stable_graph::StableDiGraph,
    visit::{Dfs, EdgeRef, IntoNodeIdentifiers, Reversed, VisitMap, Visitable},
    Direction,
};
use semver::Version;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fmt::{self, Write},
    ops::Index,
};
use thiserror::Error;
use wac_types::{
    BorrowedKey, BorrowedPackageKey, DefinedType, ItemKind, Package, PackageKey, SubtypeChecker,
    Type, TypeAggregator, Types, ValueType,
};
use wasm_encoder::{
    Alias, ComponentBuilder, ComponentExportKind, ComponentExternName, ComponentNameSection,
    ComponentTypeRef, ComponentValType, NameMap, TypeBounds,
};
use wasmparser::{
    names::{ComponentName, ComponentNameKind},
    BinaryReaderError, Validator, WasmFeatures,
};

/// Represents an error that can occur when defining a type in
/// a composition graph.
#[derive(Debug, Error)]
pub enum DefineTypeError {
    /// The provided type has already been defined in the graph.
    #[error("the provided type has already been defined in the graph")]
    TypeAlreadyDefined,
    /// A resource type cannot be defined in the graph.
    #[error("a resource type cannot be defined in the graph")]
    CannotDefineResource,
    /// The specified type name conflicts with an existing export.
    #[error("type name `{name}` conflicts with an existing export of the same name")]
    ExportConflict {
        /// The name of the existing export.
        name: String,
    },
    /// The specified type name is not a valid extern name.
    #[error("type name `{name}` is not valid")]
    InvalidExternName {
        /// The name of the type.
        name: String,
        /// The underlying validation error.
        #[source]
        source: anyhow::Error,
    },
}

/// Represents an error that can occur when importing an item in
/// a composition graph.
#[derive(Debug, Error)]
pub enum ImportError {
    /// An import name already exists in the graph.
    #[error("import name `{name}` already exists in the graph")]
    ImportAlreadyExists {
        /// The name of the existing extern.
        name: String,
        /// The node that is already imported by that name.
        node: NodeId,
    },
    /// An invalid import name was given.
    #[error("import name `{name}` is not valid")]
    InvalidImportName {
        /// The invalid name.
        name: String,
        /// The underlying validation error.
        #[source]
        source: anyhow::Error,
    },
}

/// Represents an error that can occur when registering a
/// package with a composition graph.
#[derive(Debug, Error)]
pub enum RegisterPackageError {
    /// The package is already registered in the graph.
    #[error("package `{key}` has already been registered in the graph")]
    PackageAlreadyRegistered {
        /// The key representing the already registered package
        key: PackageKey,
    },
}

/// Represents an error that can occur when aliasing an instance
/// export in a composition graph.
#[derive(Debug, Error)]
pub enum AliasError {
    /// The provided source node is not an instance.
    #[error("expected source node to be an instance, but the node is a {kind}")]
    NodeIsNotAnInstance {
        /// The source node that is not an instance.
        node: NodeId,
        /// The kind of the node.
        kind: String,
    },
    /// The instance does not export an item with the given name.
    #[error("instance does not have an export named `{export}`")]
    InstanceMissingExport {
        /// The instance node that is missing the export.
        node: NodeId,
        /// The export that was missing.
        export: String,
    },
}

/// Represents an error that can occur when exporting a node from
/// a composition graph.
#[derive(Debug, Error)]
pub enum ExportError {
    /// An export name already exists in the graph.
    #[error("an export with the name `{name}` already exists")]
    ExportAlreadyExists {
        /// The name of the existing export.
        name: String,
        /// The node that is already exported with that name.
        node: NodeId,
    },
    /// An invalid export name was given.
    #[error("export name `{name}` is not valid")]
    InvalidExportName {
        /// The invalid name.
        name: String,
        /// The underlying validation error.
        #[source]
        source: anyhow::Error,
    },
}

/// Represents an error that can occur when unexporting a node in
/// a composition graph.
#[derive(Debug, Error)]
pub enum UnexportError {
    /// The node cannot be unexported as it is a type definition.
    #[error("cannot unexport a type definition")]
    MustExportDefinition,
}

/// Represents an error that can occur when setting an instantiation
/// argument in a composition graph.
#[derive(Debug, Error)]
pub enum InstantiationArgumentError {
    /// The node is not an instantiation.
    #[error("the specified node is not an instantiation")]
    NodeIsNotAnInstantiation {
        /// The node that is not an instantiation.
        node: NodeId,
    },
    /// The provided argument name is invalid.
    #[error("argument name `{name}` is not an import of package `{package}`")]
    InvalidArgumentName {
        /// The instantiation node that does not have the provided argument name.
        node: NodeId,
        /// The name of the invalid argument.
        name: String,
        /// The name of the package that does not have the argument.
        package: String,
    },
    /// The source type does not match the target argument type.
    #[error("mismatched instantiation argument `{name}`")]
    ArgumentTypeMismatch {
        /// The name of the argument.
        name: String,
        /// The source of the error.
        #[source]
        source: anyhow::Error,
    },
    /// The argument has been passed to the instantiation.
    #[error("argument `{name}` has already been passed to the instantiation")]
    ArgumentAlreadyPassed {
        /// The instantiation node.
        node: NodeId,
        /// The name of the argument.
        name: String,
    },
}

/// Represents an error that can occur when encoding a composition graph.
#[derive(Debug, Error)]
pub enum EncodeError {
    /// The encoding of the graph failed validation.
    #[error("encoding produced a component that failed validation")]
    ValidationFailure {
        /// The source of the validation error.
        #[source]
        source: BinaryReaderError,
    },
    /// The graph contains a cycle.
    #[error("the graph contains a cycle and cannot be encoded")]
    GraphContainsCycle {
        /// The node where the cycle was detected.
        node: NodeId,
    },
    /// An implicit import on an instantiation conflicts with an explicit import node.
    #[error("an instantiation of package `{package}` implicitly imports an item named `{name}`, but it conflicts with an explicit import of the same name")]
    ImplicitImportConflict {
        /// The existing import node.
        import: NodeId,
        /// The instantiation node with the implicit import
        instantiation: NodeId,
        /// The package associated with the instantiation node.
        package: PackageKey,
        /// The name of the conflicting import.
        name: String,
    },
    /// An error occurred while merging an implicit import type.
    #[error("failed to merge the type definition for implicit import `{import}` due to conflicting types")]
    ImportTypeMergeConflict {
        /// The name of the import.
        import: String,
        /// The first conflicting instantiation node that introduced the implicit import.
        first: NodeId,
        /// The second conflicting instantiation node.
        second: NodeId,
        /// The type merge error.
        #[source]
        source: anyhow::Error,
    },
}

/// Represents an error that can occur when decoding a composition graph.
#[derive(Debug, Error)]
pub enum DecodeError {}

/// An identifier for a package in a composition graph.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct PackageId {
    /// The index into the graph's packages list.
    index: usize,
    /// The generation of the package.
    ///
    /// This is used to invalidate identifiers on package removal.
    generation: usize,
}

/// An identifier for a node in a composition graph.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct NodeId(NodeIndex);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.index().fmt(f)
    }
}

/// Represents the kind of a node in a composition graph.
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// The node is a type definition.
    Definition,
    /// The node is an import of an item.
    ///
    /// The string is the import name.
    Import(String),
    /// The node is an instantiation of a package.
    ///
    /// The set is the currently satisfied argument indexes.
    Instantiation(HashSet<usize>),
    /// The node is an alias of an export of an instance.
    Alias,
}

/// Represents a package registered with a composition graph.
#[derive(Debug, Clone)]
struct RegisteredPackage {
    /// The underlying package.
    package: Option<Package>,
    /// The generation of the package.
    ///
    /// The generation is incremented each time a package is removed from the graph.
    ///
    /// This ensures referring package identifiers are invalidated when a package is removed.
    generation: usize,
}

impl RegisteredPackage {
    fn new(generation: usize) -> Self {
        Self {
            package: None,
            generation,
        }
    }
}

/// Represents a node in a composition graph.
#[derive(Debug, Clone)]
pub struct Node {
    /// The node kind.
    kind: NodeKind,
    /// The package associated with the node, if any.
    package: Option<PackageId>,
    /// The item kind of the node.
    item_kind: ItemKind,
    /// The optional name to associate with the node.
    ///
    /// When the graph is encoded, node names are recorded in a `names` custom section.
    name: Option<String>,
    /// The name to use for exporting the node.
    export: Option<String>,
}

impl Node {
    fn new(kind: NodeKind, item_kind: ItemKind, package: Option<PackageId>) -> Self {
        Self {
            kind,
            item_kind,
            package,
            name: None,
            export: None,
        }
    }

    /// Gets the kind of the node.
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Gets the package id associated with the node.
    ///
    /// Returns `None` if the node is not directly associated with a package.
    pub fn package(&self) -> Option<PackageId> {
        self.package
    }

    /// Gets the item kind of the node.
    pub fn item_kind(&self) -> ItemKind {
        self.item_kind
    }

    /// Gets the name of the node.
    ///
    /// Node names are encoded in a `names` custom section.
    ///
    /// Returns `None` if the node is unnamed.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Gets the import name of the node.
    ///
    /// Returns `Some` if the node is an import or `None` if the node is not an import.
    pub fn import_name(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Import(name) => Some(name),
            _ => None,
        }
    }

    /// Gets the export name of the node.
    ///
    /// Returns `None` if the node is not exported.
    pub fn export_name(&self) -> Option<&str> {
        self.export.as_deref()
    }

    fn add_satisfied_arg(&mut self, index: usize) {
        match &mut self.kind {
            NodeKind::Instantiation(satisfied) => {
                let inserted = satisfied.insert(index);
                assert!(inserted);
            }
            _ => panic!("expected the node to be an instantiation"),
        }
    }

    fn remove_satisfied_arg(&mut self, index: usize) {
        match &mut self.kind {
            NodeKind::Instantiation(satisfied) => {
                let removed = satisfied.remove(&index);
                assert!(removed);
            }
            _ => panic!("expected the node to be an instantiation"),
        }
    }

    fn is_arg_satisfied(&self, index: usize) -> bool {
        match &self.kind {
            NodeKind::Instantiation(satisfied) => satisfied.contains(&index),
            _ => panic!("expected the node to be an instantiation"),
        }
    }
}

/// Represents an edge in a composition graph.
#[derive(Clone)]
enum Edge {
    /// The edge is from an instance to an aliased exported item.
    ///
    /// The data is the index of the export on the source node.
    Alias(usize),
    /// The edge is from any node to an instantiation node.
    ///
    /// The data is the import index on the instantiation node being
    /// satisfied by the argument.
    Argument(usize),
    /// A dependency from one type to another.
    Dependency,
}

impl fmt::Debug for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Edge::Alias(_) => write!(f, "aliased export"),
            Edge::Argument(_) => write!(f, "argument to"),
            Edge::Dependency => write!(f, "dependency of"),
        }
    }
}

/// Represents a composition graph.
///
/// A composition graph is a directed acyclic graph (DAG) that represents a WebAssembly composition.
///
/// Types may be defined directly in a composition graph or referenced from packages registered with
/// the graph.
///
/// There are four supported node types:
///
/// * a *type definition node* representing the definition of an exported named type.
/// * an *import node* representing an imported item.
/// * an *instantiation node* representing an instantiation of a package.
/// * an *alias node* representing an alias of an exported item from an instance.
///
/// There are three supported edge types:
///
/// * a *type dependency* edge from a type definition node to any dependent defined types;
///   type dependency edges are implicitly added to the graph when a type is defined.
/// * an *alias edge* from an any node that is an instance to an alias node; alias edges are
///   implicitly added to the graph when an alias node is added.
/// * an *instantiation argument edge* from any node to an instantiation node; instantiation
///   argument edges contain a set of instantiation arguments satisfied by the target node;
///   unsatisfied arguments are automatically imported when encoding the composition graph.
///
/// Any node in the graph may be marked to be exported from the encoded graph; note that
/// type definition nodes are _always_ exported and may not be unmarked.
#[derive(Default, Clone)]
pub struct CompositionGraph {
    /// The underlying graph data structure.
    graph: StableDiGraph<Node, Edge>,
    /// The map of import names to node indices.
    imports: HashMap<String, NodeIndex>,
    /// The map of export names to node ids.
    exports: IndexMap<String, NodeIndex>,
    /// The map of defined types to node ids.
    defined: HashMap<Type, NodeIndex>,
    /// The map of package keys to package ids.
    package_map: HashMap<PackageKey, PackageId>,
    /// The registered packages.
    packages: Vec<RegisteredPackage>,
    /// The list of free entries in the packages list.
    free_packages: Vec<usize>,
    /// The types that are directly defined by the graph.
    types: Types,
    /// The cache used for subtype checks.
    type_check_cache: HashSet<(ItemKind, ItemKind)>,
}

impl CompositionGraph {
    /// Creates a new composition graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets a reference to the type collection of the graph.
    pub fn types(&self) -> &Types {
        &self.types
    }

    /// Gets a mutable reference to the type collection of the graph.
    ///
    /// This type collection is used to define types directly in the graph.
    pub fn types_mut(&mut self) -> &mut Types {
        &mut self.types
    }

    /// Gets the nodes in the graph.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> + '_ {
        self.graph.node_weights()
    }

    /// Gets the identifiers for every node in the graph.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.graph.node_indices().map(NodeId)
    }

    /// Gets the packages currently registered with the graph.
    pub fn packages(&self) -> impl Iterator<Item = &Package> + '_ {
        self.packages.iter().filter_map(|p| p.package.as_ref())
    }

    /// Registers a package with the graph.
    ///
    /// Packages are used to create instantiations, but are not directly
    /// a part of the graph.
    ///
    /// # Panics
    ///
    /// Panics if the given package's type is not contained within the
    /// graph's types collection.
    pub fn register_package(
        &mut self,
        package: wac_types::Package,
    ) -> Result<PackageId, RegisterPackageError> {
        let key = PackageKey::new(&package);
        if self.package_map.contains_key(&key) {
            return Err(RegisterPackageError::PackageAlreadyRegistered { key });
        }

        assert!(
            self.types.contains(Type::World(package.ty())),
            "the package type is not present in the types collection"
        );

        log::debug!("registering package `{key}` with the graph");
        let id = self.alloc_package(package);
        let prev = self.package_map.insert(key, id);
        assert!(prev.is_none());
        Ok(id)
    }

    /// Unregisters a package from the graph.
    ///
    /// Any edges and nodes associated with the package are also removed.
    ///
    /// # Panics
    ///
    /// Panics if the given package identifier is invalid.
    pub fn unregister_package(&mut self, package: PackageId) {
        assert_eq!(
            self.packages
                .get(package.index)
                .expect("invalid package id")
                .generation,
            package.generation,
            "invalid package id"
        );

        // Remove exports and definitions associated with the package before
        // removing nodes, as retain_nodes invalidates the node indices.
        self.exports
            .retain(|_, n| self.graph[*n].package != Some(package));
        self.defined
            .retain(|_, n| self.graph[*n].package != Some(package));
        self.imports
            .retain(|_, n| self.graph[*n].package != Some(package));

        // Remove all nodes associated with the package
        self.graph
            .retain_nodes(|g, i| g[i].package != Some(package));

        // Remove the package from the package map
        let entry = &mut self.packages[package.index];
        let key = entry.package.as_ref().unwrap().key();
        log::debug!("unregistering package `{key}` with the graph");
        let prev = self.package_map.remove(&key as &dyn BorrowedKey);
        assert!(
            prev.is_some(),
            "package should exist in the package map (this is a bug)"
        );

        // Finally free the package
        *entry = RegisteredPackage::new(entry.generation.wrapping_add(1));
        self.free_packages.push(package.index);
    }

    /// Gets the registered package of the given package name and version.
    ///
    /// Returns `None` if a package with the specified name and version has
    /// not been registered with the graph.
    pub fn get_package_by_name(
        &self,
        name: &str,
        version: Option<&Version>,
    ) -> Option<(PackageId, &wac_types::Package)> {
        let key: BorrowedPackageKey<'_> = BorrowedPackageKey::from_name_and_version(name, version);
        let id = self.package_map.get(&key as &dyn BorrowedKey)?;
        Some((*id, self.packages[id.index].package.as_ref().unwrap()))
    }

    /// Adds a *type definition node* to the graph.
    ///
    /// The graph must not already have a node exported with the same name.
    ///
    /// This method will implicitly add dependency edges to other defined
    /// types.
    ///
    /// If the provided type has already been defined, the previous node
    /// will be returned and an additional export name will be associated
    /// with the node.
    ///
    /// # Panics
    ///
    /// This method panics if the provided type is not contained within the
    /// graph's types collection.
    pub fn define_type(
        &mut self,
        name: impl Into<String>,
        ty: Type,
    ) -> Result<NodeId, DefineTypeError> {
        assert!(
            self.types.contains(ty),
            "type not contained in types collection"
        );

        if self.defined.contains_key(&ty) {
            return Err(DefineTypeError::TypeAlreadyDefined);
        }

        if let Type::Resource(_) = ty {
            return Err(DefineTypeError::CannotDefineResource);
        }

        let name = name.into();
        if self.exports.contains_key(&name) {
            return Err(DefineTypeError::ExportConflict { name });
        }

        // Ensure that the given name is a valid extern name
        ComponentName::new(&name, 0).map_err(|e| {
            let msg = e.to_string();
            DefineTypeError::InvalidExternName {
                name: name.to_string(),
                source: anyhow::anyhow!(
                    "{msg}",
                    msg = msg.strip_suffix(" (at offset 0x0)").unwrap_or(&msg)
                ),
            }
        })?;

        let mut node = Node::new(NodeKind::Definition, ItemKind::Type(ty), None);
        node.export = Some(name.clone());
        let index = self.graph.add_node(node);
        log::debug!(
            "adding type definition `{name}` to the graph as node index {index}",
            index = index.index()
        );

        // Add dependency edges between the given type and any referenced defined types
        ty.visit_defined_types(&self.types, &mut |_, id| {
            let dep_ty = Type::Value(ValueType::Defined(id));
            if dep_ty != ty {
                if let Some(dep) = self.defined.get(&dep_ty) {
                    if !self
                        .graph
                        .edges_connecting(*dep, index)
                        .any(|e| matches!(e.weight(), Edge::Dependency))
                    {
                        log::debug!(
                            "adding dependency edge from type `{from}` (dependency) to type `{name}` (dependent)",
                            from = self.graph[*dep].export.as_ref().unwrap()
                        );
                        self.graph.add_edge(*dep, index, Edge::Dependency);
                    }
                }
            }

            Ok(())
        })?;

        // Add dependency edges to any existing defined types that reference this one
        for (other_ty, other) in &self.defined {
            other_ty.visit_defined_types(&self.types, &mut |_, id| {
                let dep_ty = Type::Value(ValueType::Defined(id));
                if dep_ty == ty
                    && !self
                        .graph
                        .edges_connecting(index, *other)
                        .any(|e| matches!(e.weight(), Edge::Dependency))
                {
                    log::debug!(
                        "adding dependency edge from type `{name}` (dependency) to type `{to}` (dependent)",
                        to = self.graph[index].export.as_ref().unwrap(),
                    );
                    self.graph.add_edge(index, *other, Edge::Dependency);
                }

                Ok(())
            })?;
        }

        self.defined.insert(ty, index);
        let prev = self.exports.insert(name, index);
        assert!(prev.is_none());
        Ok(NodeId(index))
    }

    /// Adds an *import node* to the graph.
    ///
    /// If the provided import name is invalid or if an import already exists
    /// with the same name, an error is returned.
    ///
    /// # Panics
    ///
    /// This method panics if the provided kind is not contained within the
    /// graph's types collection.
    pub fn import(
        &mut self,
        name: impl Into<String>,
        kind: ItemKind,
    ) -> Result<NodeId, ImportError> {
        assert!(
            self.types.contains(kind.ty()),
            "provided type is not in the types collection"
        );

        let name = name.into();
        if let Some(existing) = self.imports.get(&name) {
            return Err(ImportError::ImportAlreadyExists {
                name,
                node: NodeId(*existing),
            });
        }

        // Ensure that the given import name is a valid extern name
        ComponentName::new(&name, 0).map_err(|e| {
            let msg = e.to_string();
            ImportError::InvalidImportName {
                name: name.to_string(),
                source: anyhow::anyhow!(
                    "{msg}",
                    msg = msg.strip_suffix(" (at offset 0x0)").unwrap_or(&msg)
                ),
            }
        })?;

        let node = Node::new(NodeKind::Import(name.clone()), kind, None);
        let index = self.graph.add_node(node);
        log::debug!(
            "adding import `{name}` to the graph as node index {index}",
            index = index.index()
        );
        let prev = self.imports.insert(name, index);
        assert!(prev.is_none());
        Ok(NodeId(index))
    }

    /// Gets the name used by an import node.
    ///
    /// Returns `None` if the specified node is not an import node.
    ///
    /// # Panics
    ///
    /// Panics if the specified node id is invalid.
    pub fn get_import_name(&self, node: NodeId) -> Option<&str> {
        let node = self.graph.node_weight(node.0).expect("invalid node id");
        match &node.kind {
            NodeKind::Import(name) => Some(name),
            _ => None,
        }
    }

    /// Adds an *instantiation node* to the graph.
    ///
    /// Initially the instantiation will have no satisfied arguments.
    ///
    /// Use `set_instantiation_argument` to set an argument on an instantiation node.
    ///
    /// # Panics
    ///
    /// This method panics if the provided package id is invalid.
    pub fn instantiate(&mut self, package: PackageId) -> NodeId {
        let pkg = &self[package];
        let node = Node::new(
            NodeKind::Instantiation(Default::default()),
            ItemKind::Instance(pkg.instance_type()),
            Some(package),
        );
        let index = self.graph.add_node(node);
        log::debug!(
            "adding instantiation of package `{key}` to the graph as node index {index}",
            key = self[package].key(),
            index = index.index()
        );
        NodeId(index)
    }

    /// Adds an *alias node* to the graph.
    ///
    /// The provided node must be an instance and the export name must match an export
    /// of the instance.
    ///
    /// If an alias already exists for the export, the existing alias node will be returned.
    ///
    /// An implicit *alias edge* will be added from the instance to the alias node.
    ///
    /// # Panics
    ///
    /// Panics if the provided node id is invalid.
    pub fn alias_instance_export(
        &mut self,
        instance: NodeId,
        export: &str,
    ) -> Result<NodeId, AliasError> {
        let instance_node = self.graph.node_weight(instance.0).expect("invalid node id");

        // Ensure the source is an instance
        let exports = match instance_node.item_kind {
            ItemKind::Instance(id) => &self.types[id].exports,
            _ => {
                return Err(AliasError::NodeIsNotAnInstance {
                    node: instance,
                    kind: instance_node.item_kind.desc(&self.types).to_string(),
                });
            }
        };

        // Ensure the export exists
        let (index, _, kind) =
            exports
                .get_full(export)
                .ok_or_else(|| AliasError::InstanceMissingExport {
                    node: instance,
                    export: export.to_string(),
                })?;

        // Check to see if there already is an edge for this alias
        for e in self.graph.edges_directed(instance.0, Direction::Outgoing) {
            assert_eq!(e.source(), instance.0);
            if let Edge::Alias(i) = e.weight() {
                if *i == index {
                    return Ok(NodeId(e.target()));
                }
            }
        }

        // Allocate the alias node
        let node = Node::new(NodeKind::Alias, *kind, instance_node.package);
        let node_index = self.graph.add_node(node);
        log::debug!(
            "adding alias for export `{export}` to the graph as node index {index}",
            index = node_index.index()
        );
        self.graph
            .add_edge(instance.0, node_index, Edge::Alias(index));
        Ok(NodeId(node_index))
    }

    /// Gets the source node and export name of an alias node.
    ///
    /// Returns `None` if the node is not an alias.
    pub fn get_alias_source(&self, alias: NodeId) -> Option<(NodeId, &str)> {
        for e in self.graph.edges_directed(alias.0, Direction::Incoming) {
            assert_eq!(e.target(), alias.0);

            if let Edge::Alias(index) = e.weight() {
                match self.graph[e.source()].item_kind {
                    ItemKind::Instance(id) => {
                        let export = self.types[id].exports.get_index(*index).unwrap().0;
                        return Some((NodeId(e.source()), export));
                    }
                    _ => panic!("alias source should be an instance"),
                }
            }
        }

        None
    }

    /// Gets the satisfied arguments of an instantiation node.
    ///
    /// Returns an iterator over the argument names and the node id used to satisfy the argument.
    ///
    /// If the node identifier is invalid or if the node is not an instantiation node, an
    /// empty iterator is returned.
    pub fn get_instantiation_arguments(
        &self,
        instantiation: NodeId,
    ) -> impl Iterator<Item = (&str, NodeId)> {
        self.graph
            .edges_directed(instantiation.0, Direction::Incoming)
            .filter_map(|e| {
                let target = &self.graph[e.target()];
                let imports = match target.kind {
                    NodeKind::Instantiation(_) => {
                        let package = &self.packages[target.package?.index].package.as_ref()?;
                        &self.types[package.ty()].imports
                    }
                    _ => return None,
                };

                match e.weight() {
                    Edge::Alias(_) | Edge::Dependency => None,
                    Edge::Argument(i) => Some((
                        imports.get_index(*i).unwrap().0.as_ref(),
                        NodeId(e.source()),
                    )),
                }
            })
    }

    /// Sets the name of a node in the graph.
    ///
    /// Node names are recorded in a `names` custom section when the graph is encoded.
    ///
    /// # Panics
    ///
    /// This method panics if the provided node id is invalid.
    pub fn set_node_name(&mut self, node: NodeId, name: impl Into<String>) {
        let node = &mut self.graph[node.0];
        node.name = Some(name.into());
    }

    /// Marks the given node for export when the composition graph is encoded.
    ///
    /// Returns an error if the provided export name is invalid.
    ///
    /// # Panics
    ///
    /// This method panics if the provided node id is invalid.
    pub fn export(&mut self, node: NodeId, name: impl Into<String>) -> Result<(), ExportError> {
        let name = name.into();
        if let Some(existing) = self.exports.get(&name) {
            return Err(ExportError::ExportAlreadyExists {
                name,
                node: NodeId(*existing),
            });
        }

        let map_reader_err = |e: BinaryReaderError| {
            let msg = e.to_string();
            ExportError::InvalidExportName {
                name: name.to_string(),
                source: anyhow::anyhow!(
                    "{msg}",
                    msg = msg.strip_suffix(" (at offset 0x0)").unwrap_or(&msg)
                ),
            }
        };

        // Ensure that the given export name is a valid extern name
        match ComponentName::new(&name, 0).map_err(map_reader_err)?.kind() {
            ComponentNameKind::Hash(_)
            | ComponentNameKind::Url(_)
            | ComponentNameKind::Dependency(_) => {
                return Err(ExportError::InvalidExportName {
                    name: name.to_string(),
                    source: anyhow::anyhow!("export name cannot be a hash, url, or dependency"),
                });
            }
            _ => {}
        };

        log::debug!("exporting node {index} as `{name}`", index = node.0.index());
        self.graph[node.0].export = Some(name.clone());
        let prev = self.exports.insert(name, node.0);
        assert!(prev.is_none());
        Ok(())
    }

    /// Gets the node being exported by the given name.
    ///
    /// Returns `None` if there is no node exported by that name.
    pub fn get_export(&self, name: &str) -> Option<NodeId> {
        self.exports.get(name).map(|i| NodeId(*i))
    }

    /// Unmarks the given node from being exported from an encoding of the graph.
    ///
    /// Returns an error if the given node is a type definition, as type
    /// definitions must be exported.
    ///
    /// # Panics
    ///
    /// This method panics if the provided node id is invalid.
    pub fn unexport(&mut self, node: NodeId) -> Result<(), UnexportError> {
        let node = &mut self.graph[node.0];
        if let NodeKind::Definition = node.kind {
            return Err(UnexportError::MustExportDefinition);
        }

        if let Some(name) = node.export.take() {
            log::debug!("unmarked node for export as `{name}`");
            let removed = self.exports.swap_remove(&name);
            assert!(removed.is_some());
        }

        Ok(())
    }

    /// Removes a node from the graph.
    ///
    /// All incoming and outgoing edges of the node are also removed.
    ///
    /// If the node has dependent defined types, the dependent define
    /// types are also removed.
    ///
    /// If the node has aliases, the aliased nodes are also removed.
    ///
    /// # Panics
    ///
    /// This method panics if the provided node id is invalid.
    pub fn remove_node(&mut self, node: NodeId) {
        // Recursively remove any dependent nodes
        for node in self
            .graph
            .edges_directed(node.0, Direction::Outgoing)
            .filter_map(|e| {
                assert_eq!(e.source(), node.0);
                match e.weight() {
                    Edge::Alias(_) | Edge::Dependency => Some(NodeId(e.target())),
                    Edge::Argument(_) => None,
                }
            })
            .collect::<Vec<_>>()
        {
            self.remove_node(node);
        }

        // Remove the node from the graph
        log::debug!(
            "removing node {index} from the graph",
            index = node.0.index()
        );
        let node = self.graph.remove_node(node.0).expect("invalid node id");

        // Remove any import entry
        if let Some(name) = node.import_name() {
            log::debug!("removing import node `{name}`");
            let removed = self.imports.remove(name);
            assert!(removed.is_some());
        }

        // Remove any export entry
        if let Some(name) = &node.export {
            log::debug!("removing export of node as `{name}`");
            let removed = self.exports.swap_remove(name);
            assert!(removed.is_some());
        }

        if let NodeKind::Definition = node.kind {
            log::debug!(
                "removing type definition `{name}`",
                name = node.export.as_ref().unwrap()
            );
            let removed = self.defined.remove(&node.item_kind.ty());
            assert!(removed.is_some());
        }
    }

    /// Sets an argument of an instantiation node to the provided argument
    /// node.
    ///
    /// This method adds an _instantiation argument_ edge from the argument
    /// node to the instantiation node.
    ///
    /// The provided instantiation node must be an instantiation.
    ///
    /// The argument name must be a valid import on the instantiation node
    /// and not already have an incoming edge from a different argument node.
    ///
    /// The argument node must be type-compatible with the argument of the
    /// instantiation node.
    ///
    /// If an edge already exists between the argument and the instantiation
    /// node, this method returns `Ok(_)`.
    ///
    /// # Panics
    ///
    /// This method will panic if the provided node ids are invalid.
    pub fn set_instantiation_argument(
        &mut self,
        instantiation: NodeId,
        argument_name: &str,
        argument: NodeId,
    ) -> Result<(), InstantiationArgumentError> {
        fn add_edge(
            graph: &mut CompositionGraph,
            argument: NodeId,
            instantiation: NodeId,
            argument_name: &str,
            cache: &mut HashSet<(ItemKind, ItemKind)>,
        ) -> Result<(), InstantiationArgumentError> {
            // Ensure the target is an instantiation node
            let instantiation_node = &graph.graph[instantiation.0];

            if !matches!(instantiation_node.kind, NodeKind::Instantiation(_)) {
                return Err(InstantiationArgumentError::NodeIsNotAnInstantiation {
                    node: instantiation,
                });
            }

            // Ensure the argument is a valid import of the target package
            let package = graph.packages[instantiation_node.package.unwrap().index]
                .package
                .as_ref()
                .unwrap();
            let package_type = &graph.types[package.ty()];

            // Ensure the argument isn't already satisfied
            let (argument_index, _, expected_argument_kind) = package_type
                .imports
                .get_full(argument_name)
                .ok_or(InstantiationArgumentError::InvalidArgumentName {
                    node: instantiation,
                    name: argument_name.to_string(),
                    package: package.name().to_string(),
                })?;

            for e in graph
                .graph
                .edges_directed(instantiation.0, Direction::Incoming)
            {
                assert_eq!(e.target(), instantiation.0);
                match e.weight() {
                    Edge::Alias(_) | Edge::Dependency => {
                        panic!("unexpected edge for an instantiation")
                    }
                    Edge::Argument(i) => {
                        if *i == argument_index {
                            if e.source() == argument.0 {
                                return Ok(());
                            }

                            return Err(InstantiationArgumentError::ArgumentAlreadyPassed {
                                node: instantiation,
                                name: argument_name.to_string(),
                            });
                        }
                    }
                }
            }

            // Perform a subtype check on the source and target
            let argument_node = &graph.graph[argument.0];

            let mut checker = SubtypeChecker::new(cache);
            checker
                .is_subtype(
                    argument_node.item_kind,
                    &graph.types,
                    *expected_argument_kind,
                    &graph.types,
                )
                .map_err(|e| InstantiationArgumentError::ArgumentTypeMismatch {
                    name: argument_name.to_string(),
                    source: e,
                })?;

            // Finally, insert the argument edge
            graph
                .graph
                .add_edge(argument.0, instantiation.0, Edge::Argument(argument_index));

            graph.graph[instantiation.0].add_satisfied_arg(argument_index);
            Ok(())
        }

        // Temporarily take ownership of the cache to avoid borrowing issues
        let mut cache = std::mem::take(&mut self.type_check_cache);
        let result = add_edge(self, argument, instantiation, argument_name, &mut cache);
        self.type_check_cache = cache;
        result
    }

    /// Unsets an argument of an instantiation node that was previously
    /// set to the provided argument node.
    ///
    /// This method removes an _instantiation argument_ edge from the
    /// argument node to the instantiation node if the nodes are connected;
    /// if they are not connected, this method is a no-op.
    ///
    /// The provided instantiation node must be an instantiation.
    ///
    /// The argument name must be a valid import on the instantiation node.
    ///
    /// # Panics
    ///
    /// This method will panic if the provided node ids are invalid.
    pub fn unset_instantiation_argument(
        &mut self,
        instantiation: NodeId,
        argument_name: &str,
        argument: NodeId,
    ) -> Result<(), InstantiationArgumentError> {
        // Ensure the target is an instantiation node
        let instantiation_node = &self.graph[instantiation.0];
        if !matches!(instantiation_node.kind, NodeKind::Instantiation(_)) {
            return Err(InstantiationArgumentError::NodeIsNotAnInstantiation {
                node: instantiation,
            });
        }

        // Ensure the argument is a valid import of the target package
        let package = self.packages[instantiation_node.package.unwrap().index]
            .package
            .as_ref()
            .unwrap();
        let package_type = &self.types[package.ty()];

        let argument_index = package_type.imports.get_index_of(argument_name).ok_or(
            InstantiationArgumentError::InvalidArgumentName {
                node: instantiation,
                name: argument_name.to_string(),
                package: package.name().to_string(),
            },
        )?;

        // Finally remove the argument edge if a connection exists
        let mut edge = None;
        for e in self.graph.edges_connecting(argument.0, instantiation.0) {
            match e.weight() {
                Edge::Alias(_) | Edge::Dependency => {
                    panic!("unexpected edge for an instantiation")
                }
                Edge::Argument(i) => {
                    if *i == argument_index {
                        edge = Some(e.id());
                        break;
                    }
                }
            }
        }

        if let Some(edge) = edge {
            self.graph[instantiation.0].remove_satisfied_arg(argument_index);
            self.graph.remove_edge(edge);
        }

        Ok(())
    }

    /// Encodes the composition graph as a new WebAssembly component.
    ///
    /// An error will be returned if the graph contains a dependency cycle.
    pub fn encode(&self, options: EncodeOptions) -> Result<Vec<u8>, EncodeError> {
        let bytes = CompositionGraphEncoder::new(self).encode(options)?;

        if options.validate {
            Validator::new_with_features(WasmFeatures::all())
                .validate_all(&bytes)
                .map_err(|e| EncodeError::ValidationFailure { source: e })?;
        }

        Ok(bytes)
    }

    /// Decodes a composition graph from the bytes of a WebAssembly component.
    pub fn decode(_data: &[u8]) -> Result<CompositionGraph, DecodeError> {
        todo!("decoding of composition graphs is not yet implemented")
    }

    fn alloc_package(&mut self, package: wac_types::Package) -> PackageId {
        let (index, entry) = if let Some(index) = self.free_packages.pop() {
            let entry = &mut self.packages[index];
            assert!(entry.package.is_none());
            (index, entry)
        } else {
            let index = self.packages.len();
            self.packages.push(RegisteredPackage::new(0));
            (index, &mut self.packages[index])
        };

        entry.package = Some(package);

        PackageId {
            index,
            generation: entry.generation,
        }
    }

    /// Yields an iterator over the resolved imports (both implicit and explicit) of the graph.
    ///
    /// The iterator returns the name, the `ItemKind`, and an optional node id for explicit imports.
    pub fn imports(&self) -> impl Iterator<Item = (&str, ItemKind, Option<NodeId>)> {
        let mut imports = Vec::new();
        for index in self.graph.node_indices() {
            let node = &self.graph[index];
            if !matches!(node.kind, NodeKind::Instantiation(_)) {
                continue;
            }

            let package = &self[node.package.unwrap()];
            let world = &self.types[package.ty()];

            let unsatisfied_args = world
                .imports
                .iter()
                .enumerate()
                .filter(|(i, _)| !node.is_arg_satisfied(*i));

            // Go through the unsatisfied arguments and import them
            for (_, (name, item_kind)) in unsatisfied_args {
                imports.push((name.as_str(), *item_kind, None));
            }
        }

        for n in self.node_ids() {
            let node = &self.graph[n.0];
            if let NodeKind::Import(name) = &node.kind {
                imports.push((name.as_str(), node.item_kind, Some(n)));
            }
        }
        imports.into_iter()
    }
}

impl Index<NodeId> for CompositionGraph {
    type Output = Node;

    fn index(&self, index: NodeId) -> &Self::Output {
        &self.graph[index.0]
    }
}

impl Index<PackageId> for CompositionGraph {
    type Output = Package;

    fn index(&self, index: PackageId) -> &Self::Output {
        let PackageId { index, generation } = index;
        let entry = self.packages.get(index).expect("invalid package id");
        if entry.generation != generation {
            panic!("invalid package id");
        }

        entry.package.as_ref().unwrap()
    }
}

impl fmt::Debug for CompositionGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_attr = |_, (i, node): (_, &Node)| {
            let label = match &node.kind {
                NodeKind::Definition => format!(
                    r#"type definition \"{name}\""#,
                    name = node.export.as_ref().unwrap()
                ),
                NodeKind::Import(name) => format!(r#"import \"{name}\""#),
                NodeKind::Instantiation(_) => {
                    let package = &self[node.package.unwrap()];
                    format!(r#"instantiation of package \"{key}\""#, key = package.key())
                }
                NodeKind::Alias => {
                    let (_, source) = self.get_alias_source(NodeId(i)).unwrap();
                    format!(r#"alias of export \"{source}\""#)
                }
            };

            let mut desc = String::new();
            write!(
                &mut desc,
                r#"label = "{label}"; kind = "{kind}""#,
                kind = node.item_kind.desc(&self.types)
            )
            .unwrap();

            if let Some(export) = &node.export {
                write!(&mut desc, r#"; export = "{export}""#).unwrap();
            }

            desc
        };

        let dot = Dot::with_attr_getters(
            &self.graph,
            &[Config::NodeNoLabel],
            &|_, _| String::new(),
            &node_attr,
        );

        write!(f, "{:?}", dot)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
/// Information about the tool that processed the graph.
pub struct Processor<'a> {
    /// The name of the tool that processed the graph.
    pub name: &'a str,
    /// The version of the tool that processed the graph.
    pub version: &'a str,
}

/// The options for encoding a composition graph.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct EncodeOptions<'a> {
    /// Whether or not to define instantiated components.
    ///
    /// If `false`, components will be imported instead.
    ///
    /// Defaults to `true`.
    pub define_components: bool,

    /// Whether or not to validate the encoded output.
    ///
    /// Defaults to `true`.
    pub validate: bool,

    /// Information about the processor of the composition graph.
    pub processor: Option<Processor<'a>>,
}

impl Default for EncodeOptions<'_> {
    fn default() -> Self {
        Self {
            define_components: true,
            validate: true,
            processor: None,
        }
    }
}

/// Used to encode a composition graph as a new WebAssembly component.
struct CompositionGraphEncoder<'a>(&'a CompositionGraph);

impl<'a> CompositionGraphEncoder<'a> {
    fn new(graph: &'a CompositionGraph) -> Self {
        Self(graph)
    }

    fn encode(self, options: EncodeOptions) -> Result<Vec<u8>, EncodeError> {
        let mut state = State::new();

        // Separate import nodes from other nodes keeping topological order
        let (import_nodes, other_nodes) = self
            .toposort()
            .map_err(|n| EncodeError::GraphContainsCycle { node: NodeId(n) })?
            .into_iter()
            .partition::<Vec<_>, _>(|index| {
                let node = &self.0.graph[*index];
                matches!(node.kind, NodeKind::Import(_))
            });

        // First populate the state with both implicit instantiation arguments and explicit imports
        self.encode_imports(&mut state, import_nodes)?;

        // Encode non-import nodes in the graph in topographical order
        for n in other_nodes {
            let node = &self.0.graph[n];
            let index = match &node.kind {
                NodeKind::Definition => self.definition(&mut state, node),
                NodeKind::Instantiation(_) => self.instantiation(&mut state, n, node, options),
                NodeKind::Alias => self.alias(&mut state, n),
                // `other_nodes` does not contain any import nodes
                NodeKind::Import(_) => unreachable!(),
            };

            let prev = state.node_indexes.insert(n, index);
            assert!(prev.is_none());
        }

        // Encode the exports, skipping any definitions as they've
        // already been exported
        for (name, node) in self
            .0
            .exports
            .iter()
            .filter(|(_, n)| !matches!(self.0.graph[**n].kind, NodeKind::Definition))
        {
            let index = state.node_indexes[node];
            let node = &self.0.graph[*node];
            state
                .builder()
                .export(name, node.item_kind.into(), index, None);
        }

        let mut builder = std::mem::take(state.builder());
        self.encode_names(&state, &mut builder);

        if let Some(processor) = &options.processor {
            let mut section = wasm_metadata::Producers::empty();
            section.add("processed-by", processor.name, processor.version);
            builder.raw_custom_section(&section.raw_custom_section());
        }

        Ok(builder.finish())
    }

    /// Performs a toposort of the composition graph.
    ///
    /// This differs from `toposort` in `petgraph` in that the
    /// nodes are iterated in *reverse order*, resulting in the
    /// returned topologically-sorted set to be in index order for
    /// independent nodes.
    fn toposort(&self) -> Result<Vec<NodeIndex>, NodeIndex> {
        let graph = &self.0.graph;
        let mut dfs = Dfs::empty(graph);
        dfs.reset(graph);
        let mut finished = graph.visit_map();

        let mut finish_stack = Vec::new();
        for i in graph.node_identifiers().rev() {
            if dfs.discovered.is_visited(&i) {
                continue;
            }
            dfs.stack.push(i);
            while let Some(&nx) = dfs.stack.last() {
                if dfs.discovered.visit(nx) {
                    // First time visiting `nx`: Push neighbors, don't pop `nx`
                    for succ in graph.neighbors(nx) {
                        if succ == nx {
                            // self cycle
                            return Err(nx);
                        }
                        if !dfs.discovered.is_visited(&succ) {
                            dfs.stack.push(succ);
                        }
                    }
                } else {
                    dfs.stack.pop();
                    if finished.visit(nx) {
                        // Second time: All reachable nodes must have been finished
                        finish_stack.push(nx);
                    }
                }
            }
        }
        finish_stack.reverse();

        dfs.reset(graph);
        for &i in &finish_stack {
            dfs.move_to(i);
            let mut cycle = false;
            while let Some(j) = dfs.next(Reversed(graph)) {
                if cycle {
                    return Err(j);
                }
                cycle = true;
            }
        }

        Ok(finish_stack)
    }

    /// Encode both implicit and explicit imports.
    ///
    /// `import_nodes` are expected to only be `NodeKind::Import` nodes.
    fn encode_imports(
        &self,
        state: &mut State,
        import_nodes: Vec<NodeIndex>,
    ) -> Result<(), EncodeError> {
        let mut explicit_imports = HashMap::new();
        let mut implicit_imports = Vec::new();
        let aggregator =
            self.resolve_imports(import_nodes, &mut implicit_imports, &mut explicit_imports)?;

        let mut encoded = HashMap::new();

        // Instances first, so a later type import can alias a type from an
        // instance export instead of re-encoding it as a local definition.
        let (instances, rest): (Vec<_>, Vec<_>) = aggregator
            .imports()
            .partition(|(_, kind)| matches!(kind, ItemKind::Instance(_)));
        for (name, kind) in instances.into_iter().chain(rest) {
            log::debug!("import `{name}` is being imported");
            let index = self.import(state, name, aggregator.types(), kind);
            encoded.insert(name, (kind.into(), index));
        }

        // Populate the implicit argument map
        for (name, node) in implicit_imports {
            let canonical = aggregator.canonical_import_name(name);
            let (kind, index) = encoded[canonical];
            state
                .implicit_args
                .entry(node)
                .or_default()
                .push((name.to_owned(), kind, index));
        }

        // Finally, populate the node indexes with the encoded explicit imports
        for (name, node_index) in explicit_imports {
            let canonical = aggregator.canonical_import_name(name);
            let (_, encoded_index) = encoded[canonical];
            state.node_indexes.insert(node_index, encoded_index);
        }

        Ok(())
    }

    /// Resolves the imports (both implicit and explicit) of the given nodes.
    ///
    /// Populates hashmaps that map the implicit and explicit import nodes to their import names.
    /// Returns a type aggregator that contains the resolved types of the imports.
    fn resolve_imports(
        &'a self,
        import_nodes: Vec<NodeIndex>,
        implicit_imports: &mut Vec<(&'a str, NodeIndex)>,
        explicit_imports: &mut HashMap<&'a str, NodeIndex>,
    ) -> Result<TypeAggregator, EncodeError> {
        let mut instantiations = HashMap::new();
        let mut aggregator = TypeAggregator::default();
        let mut cache = Default::default();
        let mut checker = SubtypeChecker::new(&mut cache);

        log::debug!("populating implicit imports");

        // Enumerate the instantiation nodes and populate the import types
        for index in self.0.graph.node_indices() {
            let node = &self.0.graph[index];
            if !matches!(node.kind, NodeKind::Instantiation(_)) {
                continue;
            }

            let package = &self.0[node.package.unwrap()];
            let world = &self.0.types[package.ty()];

            let unsatisfied_args = world
                .imports
                .iter()
                .enumerate()
                .filter(|(i, _)| !node.is_arg_satisfied(*i));

            // Go through the unsatisfied arguments and import them
            for (_, (name, kind)) in unsatisfied_args {
                if let Some(import) = self.0.imports.get(name).copied() {
                    return Err(EncodeError::ImplicitImportConflict {
                        import: NodeId(import),
                        instantiation: NodeId(index),
                        package: PackageKey::new(package),
                        name: name.to_string(),
                    });
                }

                instantiations.entry(name).or_insert(index);

                aggregator = aggregator
                    .aggregate(name, &self.0.types, *kind, &mut checker)
                    .map_err(|e| EncodeError::ImportTypeMergeConflict {
                        import: name.clone(),
                        first: NodeId(instantiations[&name]),
                        second: NodeId(index),
                        source: e,
                    })?;
                implicit_imports.push((name, index));
            }
        }

        log::debug!("populating explicit imports");

        for n in import_nodes {
            let node = &self.0.graph[n];
            if let NodeKind::Import(name) = &node.kind {
                explicit_imports.insert(name.as_str(), n);
                aggregator = aggregator
                    .aggregate(name, self.0.types(), node.item_kind, &mut checker)
                    .unwrap();
            }
        }
        Ok(aggregator)
    }

    fn definition(&self, state: &mut State, node: &Node) -> u32 {
        let name = node.export.as_deref().unwrap();

        log::debug!(
            "encoding definition for {kind} `{name}`",
            kind = node.item_kind.desc(&self.0.types)
        );

        // Check to see if the type is already
        let encoder = TypeEncoder::new(&self.0.types);
        let (ty, index) = match node.item_kind {
            ItemKind::Type(ty) => match ty {
                Type::Resource(_) => panic!("resources cannot be defined"),
                Type::Func(_) => (ty, encoder.ty(state, ty, None)),
                Type::Value(vt) => {
                    // Check for an alias and use the existing index
                    if let ValueType::Defined(id) = vt {
                        if let DefinedType::Alias(aliased @ ValueType::Defined(_)) =
                            &self.0.types()[id]
                        {
                            if let Some(&index) =
                                // check if aliased had previously been exported
                                state.current.type_indexes.get(&Type::Value(*aliased))
                            {
                                (ty, index)
                            } else {
                                (ty, encoder.ty(state, ty, None))
                            }
                        } else {
                            (ty, encoder.ty(state, ty, None))
                        }
                    } else {
                        (ty, encoder.ty(state, ty, None))
                    }
                }
                Type::Interface(id) => (ty, encoder.interface(state, id)),
                Type::World(id) => (ty, encoder.world(state, id)),
                Type::Module(_) => (ty, encoder.ty(state, ty, None)),
            },
            _ => panic!("only types can be defined"),
        };

        let index = state
            .builder()
            .export(name, ComponentExportKind::Type, index, None);

        // Remap to the exported index
        state.current.type_indexes.insert(ty, index);

        log::debug!("definition `{name}` encoded to type index {index}");
        index
    }

    fn import(&self, state: &mut State, name: &str, types: &Types, kind: ItemKind) -> u32 {
        // Determine whether this is a component model `(implements "I")` import.
        // The same interface may be imported multiple times under different
        // labels, so implements imports are exempt from the shared-dependency
        // deduplication below and carry an `implements` directive when
        // re-encoded.
        let implements = match kind {
            ItemKind::Instance(id) => crate::encoding::implements_directive(types, name, id),
            _ => None,
        };

        // Check to see if this is an import of an interface that's already been
        // imported; this can happen based on importing of shared dependencies.
        // Implements imports are skipped here since the same interface can be
        // imported multiple times under distinct labels.
        if implements.is_none() {
            if let ItemKind::Instance(id) = kind {
                if let Some(id) = &types[id].id {
                    if let Some(index) = state.current.instances.get(id) {
                        return *index;
                    }
                }
            }
        }

        // Defer to special handling if the item being imported is a resource
        let encoder = TypeEncoder::new(types);
        if let ItemKind::Type(Type::Resource(id)) = kind {
            return encoder.import_resource(state, name, id);
        }

        log::debug!(
            "encoding import of {kind} `{name}`",
            kind = kind.desc(types)
        );

        // Encode the type and import
        let ty = encoder.ty(state, kind.ty(), None);
        let extern_name = ComponentExternName {
            name: Cow::Borrowed(name),
            implements: implements.map(Cow::Borrowed),
            external_id: None,
            version_suffix: None,
        };
        let index = state.builder().import(
            extern_name,
            match kind {
                ItemKind::Type(_) => ComponentTypeRef::Type(TypeBounds::Eq(ty)),
                ItemKind::Func(_) => ComponentTypeRef::Func(ty),
                ItemKind::Instance(_) => ComponentTypeRef::Instance(ty),
                ItemKind::Component(_) => ComponentTypeRef::Component(ty),
                ItemKind::Module(_) => ComponentTypeRef::Module(ty),
                ItemKind::Value(_) => ComponentTypeRef::Value(ComponentValType::Type(ty)),
            },
        );

        log::debug!(
            "import `{name}` encoded to {desc} index {index}",
            desc = kind.desc(types)
        );

        match kind {
            ItemKind::Type(ty) => {
                // Remap to the imported index
                state.current.type_indexes.insert(ty, index);
            }
            ItemKind::Instance(id) => {
                encoder.record_instance_resources(state, id, index);
                // Implements imports are not registered for shared-dependency
                // aliasing: distinct labels of the same interface must remain
                // distinct instances rather than collapsing to one.
                if let (None, Some(id)) = (&implements, &types[id].id) {
                    log::debug!(
                        "interface `{id}` is available for aliasing as instance index {index}"
                    );
                    state.current.instances.insert(id.clone(), index);
                }
            }
            _ => {}
        }

        index
    }

    fn instantiation(
        &self,
        state: &mut State,
        index: NodeIndex,
        node: &Node,
        options: EncodeOptions,
    ) -> u32 {
        let package_id = node.package.expect("instantiation requires a package");
        let package = self.0.packages[package_id.index].package.as_ref().unwrap();
        let imports = &self.0.types[package.ty()].imports;

        let component_index = if let Some(index) = state.packages.get(&package_id) {
            *index
        } else {
            let index = if options.define_components {
                state.builder().component_raw(None, package.bytes())
            } else {
                let encoder = TypeEncoder::new(&self.0.types);
                let ty = encoder.component(state, package.ty());
                state.builder().import(
                    Self::package_import_name(package),
                    ComponentTypeRef::Component(ty),
                )
            };

            state.packages.insert(package_id, index);
            index
        };

        let mut arguments = Vec::with_capacity(imports.len());
        arguments.extend(
            self.0
                .graph
                .edges_directed(index, Direction::Incoming)
                .map(|e| {
                    let kind = self.0.graph[e.source()].item_kind.into();
                    let index = state.node_indexes[&e.source()];
                    match e.weight() {
                        Edge::Alias(_) | Edge::Dependency => {
                            panic!("unexpected edge for an instantiation")
                        }
                        Edge::Argument(i) => (
                            Cow::Borrowed(imports.get_index(*i).unwrap().0.as_str()),
                            kind,
                            index,
                        ),
                    }
                }),
        );

        if let Some(implicit) = state.implicit_args.remove(&index) {
            arguments.extend(implicit.into_iter().map(|(n, k, i)| (n.into(), k, i)));
        }

        log::debug!(
            "instantiating package `{package}` with arguments: {arguments:?}",
            package = package.name(),
        );

        let index = state
            .builder()
            .instantiate(None, component_index, arguments);

        log::debug!(
            "instantiation of package `{package}` encoded to instance index {index}",
            package = package.name(),
        );

        index
    }

    fn alias(&self, state: &mut State, node: NodeIndex) -> u32 {
        let (source, export) = self
            .0
            .get_alias_source(NodeId(node))
            .expect("alias should have a source");

        let source_node = &self.0[source];
        let exports = match source_node.item_kind {
            ItemKind::Instance(id) => &self.0.types[id].exports,
            _ => panic!("expected the source of an alias to be an instance"),
        };

        let kind = exports[export];
        let instance = state.node_indexes[&source.0];

        log::debug!(
            "encoding alias for {kind} export `{export}` of instance index {instance}",
            kind = kind.desc(&self.0.types),
        );

        let index = state.builder().alias(
            None,
            Alias::InstanceExport {
                instance,
                kind: kind.into(),
                name: export,
            },
        );

        log::debug!(
            "alias of export `{export}` encoded to {kind} index {index}",
            kind = kind.desc(&self.0.types)
        );
        index
    }

    fn package_import_name(package: &Package) -> String {
        let mut name = String::new();

        write!(&mut name, "unlocked-dep=<{name}", name = package.name()).unwrap();
        if let Some(version) = package.version() {
            write!(&mut name, "@{{>={version}}}", version = version).unwrap();
        }

        write!(&mut name, ">").unwrap();
        name
    }

    fn encode_names(&self, state: &State, encoded: &mut ComponentBuilder) {
        let mut names = ComponentNameSection::new();
        let mut types = NameMap::new();
        let mut funcs = NameMap::new();
        let mut instances = NameMap::new();
        let mut components = NameMap::new();
        let mut modules = NameMap::new();
        let mut values = NameMap::new();

        for index in self.0.graph.node_indices() {
            let node = &self.0.graph[index];
            if let Some(name) = &node.name {
                let map = match node.item_kind {
                    ItemKind::Type(_) => &mut types,
                    ItemKind::Func(_) => &mut funcs,
                    ItemKind::Instance(_) => &mut instances,
                    ItemKind::Component(_) => &mut components,
                    ItemKind::Module(_) => &mut modules,
                    ItemKind::Value(_) => &mut values,
                };

                let index = state.node_indexes[&index];
                map.append(index, name)
            }
        }

        if !types.is_empty() {
            names.types(&types);
        }

        if !funcs.is_empty() {
            names.funcs(&funcs);
        }

        if !instances.is_empty() {
            names.instances(&instances);
        }

        if !components.is_empty() {
            names.components(&components);
        }

        if !modules.is_empty() {
            names.core_modules(&modules);
        }

        if !values.is_empty() {
            names.values(&values);
        }

        if !names.is_empty() {
            encoded.custom_section(&names.as_custom());
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use wac_types::{DefinedType, PrimitiveType, Resource, ValueType};

    #[test]
    fn it_adds_a_type_definition() {
        let mut graph = CompositionGraph::new();
        let id = graph
            .types_mut()
            .add_defined_type(DefinedType::Alias(ValueType::Primitive(
                PrimitiveType::Bool,
            )));
        assert!(graph
            .define_type("foo", Type::Value(ValueType::Defined(id)))
            .is_ok());
    }

    #[test]
    fn it_cannot_define_a_resource() {
        let mut graph = CompositionGraph::new();
        let id = graph.types_mut().add_resource(Resource {
            name: "a".to_string(),
            alias: None,
        });
        assert!(matches!(
            graph.define_type("foo", Type::Resource(id)).unwrap_err(),
            DefineTypeError::CannotDefineResource
        ));
    }

    #[test]
    fn it_must_export_a_type_definition() {
        let mut graph = CompositionGraph::new();
        let id = graph
            .types_mut()
            .add_defined_type(DefinedType::Alias(ValueType::Primitive(PrimitiveType::S32)));
        let id = graph
            .define_type("foo", Type::Value(ValueType::Defined(id)))
            .unwrap();
        assert!(matches!(
            graph.unexport(id).unwrap_err(),
            UnexportError::MustExportDefinition
        ));
    }

    #[test]
    fn it_can_remove_a_type_definition() {
        let mut graph = CompositionGraph::new();
        let ty_id = graph
            .types_mut()
            .add_defined_type(DefinedType::Alias(ValueType::Primitive(PrimitiveType::S32)));
        let node_id = graph
            .define_type("foo", Type::Value(ValueType::Defined(ty_id)))
            .unwrap();

        // Definition nodes store their name in `export`, not `name`.
        // Removing a definition node should not panic.
        graph.remove_node(node_id);

        // Verify the definition and export were cleaned up
        assert!(graph.get_export("foo").is_none());
    }
    #[test]
    fn it_cleans_up_exports_on_unregister_package() {
        let mut graph = CompositionGraph::new();
        let bytes = wat::parse_str(
            r#"(component
                (import "f" (func))
                (export "g" (func 0))
            )"#,
        )
        .unwrap();

        let package =
            wac_types::Package::from_bytes("test:pkg", None, bytes, graph.types_mut()).unwrap();
        let pkg_id = graph.register_package(package).unwrap();

        // Create an instantiation and export an alias
        let inst_id = graph.instantiate(pkg_id);
        let alias_id = graph.alias_instance_export(inst_id, "g").unwrap();
        graph.export(alias_id, "g").unwrap();

        assert!(graph.get_export("g").is_some());

        // Unregistering the package should clean up exports
        graph.unregister_package(pkg_id);
        assert!(
            graph.get_export("g").is_none(),
            "exports should be cleaned up after unregister_package"
        );
    }
}
