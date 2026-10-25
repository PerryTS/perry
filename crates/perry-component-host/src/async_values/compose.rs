//! Compose resource-typed helpers with the original component. Resource aliases
//! come from actual imports/exports, never interfaces pruned from a WIT world.
use super::{reader, reader_types::Resources};
use anyhow::{Context, Result};
use perry_wit::{Type as Wit, World};
use std::collections::{BTreeMap, BTreeSet};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};
use wasmtime::{
    component::{types::ComponentItem, Component, ResourceType},
    Engine,
};

pub(crate) const ORIGINAL: &str = "perry-original";
pub(crate) struct Prepared {
    pub component: Component,
    pub control: Option<String>,
    pub readers: Vec<String>,
    pub schemas: Vec<Wit>,
    pub disposers: Option<String>,
}
impl Prepared {
    pub fn new(
        engine: &Engine,
        bytes: &[u8],
        original: Component,
        world: &World,
        wasi: bool,
        identities: &BTreeMap<String, ResourceType>,
        disposal: bool,
    ) -> Result<Self> {
        let mut specs = Vec::new();
        let mut seen = BTreeSet::new();
        for interface in world
            .imports
            .iter()
            .filter(|i| !(wasi && i.name.starts_with("wasi:")))
            .chain(&world.exports)
        {
            for function in &interface.functions {
                for ty in function
                    .params
                    .iter()
                    .map(|f| &f.ty)
                    .chain(function.result.iter())
                {
                    collect(world, ty, &mut seen, &mut specs, 0)?;
                }
            }
        }
        if specs.is_empty() && (!disposal || identities.is_empty()) {
            return Ok(Self {
                component: original,
                control: None,
                readers: Vec::new(),
                schemas: Vec::new(),
                disposers: None,
            });
        }
        let ty = original.component_type();
        let imports: BTreeSet<_> = ty.imports(engine).map(|(n, _)| n.to_string()).collect();
        let mut names = imports.clone();
        let mut fresh = |prefix: &str| {
            let mut i = 0;
            loop {
                let name = format!("perry-{prefix}-{i}");
                if names.insert(name.clone()) {
                    break name;
                }
                i += 1;
            }
        };
        let control = fresh("async-control");
        let mut sources = Vec::new();
        for (exported, items) in [
            (
                false,
                ty.imports(engine)
                    .map(|(n, x)| (n.to_string(), x.ty))
                    .collect::<Vec<_>>(),
            ),
            (
                true,
                ty.exports(engine)
                    .map(|(n, x)| (n.to_string(), x.ty))
                    .collect(),
            ),
        ] {
            for (namespace, item) in items {
                match item {
                    ComponentItem::Resource(resource) => {
                        sources.push((resource, exported, String::new(), namespace))
                    }
                    ComponentItem::ComponentInstance(instance) => {
                        for (name, item) in instance.exports(engine) {
                            if let ComponentItem::Resource(resource) = item.ty {
                                sources.push((resource, exported, namespace.clone(), name.into()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut bindings = Resources::new();
        let mut explicit = BTreeMap::new();
        let mut export_names = BTreeMap::new();
        for (key, resource) in identities {
            let (_, exported, namespace, name) = sources
                .iter()
                .find(|(ty, _, _, _)| ty == resource)
                .with_context(|| format!("no original resource export/import for {key}"))?;
            let binding = if !exported {
                (namespace.clone(), name.clone())
            } else if namespace.is_empty() {
                let local = fresh("resource-type");
                explicit.insert(local.clone(), (String::new(), name.clone()));
                (String::new(), local)
            } else {
                let local = export_names
                    .entry(namespace.clone())
                    .or_insert_with(|| fresh("resource-exports"))
                    .clone();
                explicit.insert(local.clone(), (namespace.clone(), String::new()));
                (local, name.clone())
            };
            bindings.insert(key.clone(), binding);
        }
        let mut graph = CompositionGraph::new();
        let package = Package::from_bytes("perry:original", None, bytes, graph.types_mut())?;
        let package = graph.register_package(package)?;
        let original_node = graph.instantiate(package);
        graph.export(original_node, ORIGINAL)?;
        let mut readers = Vec::new();
        let mut schemas = Vec::new();
        for (index, (stream, payload)) in specs.into_iter().enumerate() {
            let text = reader::generate(world, payload.as_ref(), stream, &bindings, &control)?;
            let package = Package::from_bytes(
                &format!("perry:reader-{index}"),
                None,
                wat::parse_str(text)?,
                graph.types_mut(),
            )?;
            let helper_imports: BTreeSet<_> = graph.types()[package.ty()]
                .imports
                .keys()
                .cloned()
                .collect();
            let package = graph.register_package(package)?;
            let helper = graph.instantiate(package);
            // Wasmtime rejects intra-component reads/writes of non-numeric
            // payloads. Factory/writes use a distinct instance with the same
            // original resource aliases; readable transport stays here.
            let writer = graph.instantiate(package);
            for (local, (namespace, name)) in &explicit {
                if !helper_imports.contains(local) {
                    continue;
                }
                let node = graph.alias_instance_export(
                    original_node,
                    if namespace.is_empty() {
                        name
                    } else {
                        namespace
                    },
                )?;
                graph.set_instantiation_argument(helper, local, node)?;
                graph.set_instantiation_argument(writer, local, node)?;
            }
            let name = format!("perry-reader-{index}");
            graph.export(helper, &name)?;
            graph.export(writer, format!("perry-writer-{index}"))?;
            readers.push(name);
            schemas.push(if stream {
                Wit::Stream(payload.map(Box::new))
            } else {
                Wit::Future(payload.map(Box::new))
            });
        }
        let disposers = if disposal && !bindings.is_empty() {
            let text = super::resource_dispose::generate(world, &bindings)?;
            let package = Package::from_bytes(
                "perry:resource-disposal",
                None,
                wat::parse_str(text)?,
                graph.types_mut(),
            )?;
            let helper_imports: BTreeSet<_> = graph.types()[package.ty()]
                .imports
                .keys()
                .cloned()
                .collect();
            let package = graph.register_package(package)?;
            let helper = graph.instantiate(package);
            for (local, (namespace, name)) in &explicit {
                if !helper_imports.contains(local) {
                    continue;
                }
                let node = graph.alias_instance_export(
                    original_node,
                    if namespace.is_empty() {
                        name
                    } else {
                        namespace
                    },
                )?;
                graph.set_instantiation_argument(helper, local, node)?;
            }
            let name = "perry-resource-disposal";
            graph.export(helper, name)?;
            Some(name.to_string())
        } else {
            None
        };
        let bytes = graph.encode(EncodeOptions::default())?;
        Ok(Self {
            component: Component::from_binary(engine, &bytes)
                .map_err(anyhow::Error::from)
                .context("compiling generic async composition")?,
            control: Some(control),
            readers,
            schemas,
            disposers,
        })
    }
    pub fn original(&self) -> Option<&str> {
        self.control.as_ref().map(|_| ORIGINAL)
    }
}

fn collect(
    world: &World,
    ty: &Wit,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<(bool, Option<Wit>)>,
    depth: usize,
) -> Result<()> {
    anyhow::ensure!(depth < 128, "async type nesting exceeds 128");
    match ty {
        Wit::Ref(key) => {
            if seen.insert(key.clone()) {
                collect(
                    world,
                    &world.types.get(key).context("unknown async type")?.ty,
                    seen,
                    out,
                    depth + 1,
                )?;
            }
        }
        Wit::Future(payload) | Wit::Stream(payload) => {
            if let Some(payload) = payload {
                collect(world, payload, seen, out, depth + 1)?;
            }
            let item = (matches!(ty, Wit::Stream(_)), payload.as_deref().cloned());
            if !out.contains(&item) {
                out.push(item);
            }
        }
        Wit::Record(fields) => {
            for f in fields {
                collect(world, &f.ty, seen, out, depth + 1)?;
            }
        }
        Wit::Tuple(fields) => {
            for f in fields {
                collect(world, f, seen, out, depth + 1)?;
            }
        }
        Wit::List(ty) | Wit::Option(ty) => collect(world, ty, seen, out, depth + 1)?,
        Wit::Result { ok, err } => {
            for ty in ok.iter().chain(err).map(|t| t.as_ref()) {
                collect(world, ty, seen, out, depth + 1)?;
            }
        }
        Wit::Variant(cases) => {
            for ty in cases.iter().filter_map(|c| c.ty.as_ref()) {
                collect(world, ty, seen, out, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
