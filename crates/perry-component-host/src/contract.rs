use anyhow::{bail, ensure, Context, Result};
use perry_wit::{Function, Interface, Type as Wit, World, SCHEMA_VERSION};
use std::collections::BTreeMap;
use wasmtime::component::{types::ComponentItem, Component, ResourceType, Type};
use wasmtime::Engine;

/// Validate the selected world before instantiation can execute component code.
/// Names are looked up exactly, so loading a different interface version cannot
/// silently substitute a semver-compatible overload from the linker.
pub(crate) fn validate(
    engine: &Engine,
    component: &Component,
    world: &World,
    wasi_enabled: bool,
) -> Result<BTreeMap<String, ResourceType>> {
    ensure!(
        world.schema_version == SCHEMA_VERSION,
        "unsupported WIT manifest version"
    );
    let component = component.component_type();
    let imports = component
        .imports(engine)
        .map(|(n, x)| (n.to_string(), x.ty))
        .collect();
    let exports = component
        .exports(engine)
        .map(|(n, x)| (n.to_string(), x.ty))
        .collect();
    let mut checker = Checker {
        world,
        resources: BTreeMap::new(),
    };
    checker
        .interfaces(engine, &world.imports, &imports, wasi_enabled)
        .context("component imports do not match WIT")?;
    checker
        .interfaces(engine, &world.exports, &exports, false)
        .context("component exports do not match WIT")?;
    Ok(checker.resources)
}

struct Checker<'a> {
    world: &'a World,
    resources: BTreeMap<String, ResourceType>,
}

impl Checker<'_> {
    fn interfaces(
        &mut self,
        engine: &Engine,
        expected: &[Interface],
        actual: &BTreeMap<String, ComponentItem>,
        prune_wasi_imports: bool,
    ) -> Result<()> {
        for interface in expected {
            // Componentization removes unused WASI imports. Providers supply
            // the full interface, so an unused member need not remain in the
            // binary. Application contracts and all exports stay strict.
            let pruned = prune_wasi_imports && interface.name.starts_with("wasi:");
            if pruned && !actual.contains_key(&interface.name) {
                let family = interface.name.split('@').next();
                ensure!(
                    !actual.keys().any(|name| name.split('@').next() == family),
                    "missing interface {} (component requires a different version)",
                    interface.name
                );
                continue;
            }
            let items;
            let actual = if interface.name.is_empty() {
                actual
            } else {
                let ComponentItem::ComponentInstance(instance) = actual
                    .get(&interface.name)
                    .with_context(|| format!("missing interface {}", interface.name))?
                else {
                    bail!("expected interface {}", interface.name);
                };
                items = instance
                    .exports(engine)
                    .map(|(n, x)| (n.to_string(), x.ty))
                    .collect();
                &items
            };
            for (name, key) in &interface.resources {
                if pruned && !actual.contains_key(name) {
                    continue;
                }
                let Some(ComponentItem::Resource(ty)) = actual.get(name) else {
                    bail!("missing resource {name}");
                };
                self.resource(key, *ty)?;
            }
            for function in &interface.functions {
                if pruned && !actual.contains_key(&function.name) {
                    continue;
                }
                self.function(
                    function,
                    actual
                        .get(&function.name)
                        .with_context(|| format!("missing function {}", function.name))?,
                )
                .with_context(|| format!("{} / {}", interface.name, function.name))?;
            }
        }
        Ok(())
    }

    fn function(&mut self, expected: &Function, actual: &ComponentItem) -> Result<()> {
        let ComponentItem::ComponentFunc(actual) = actual else {
            bail!("expected component function");
        };
        ensure!(
            expected.async_ == actual.async_(),
            "async calling convention mismatch"
        );
        let params: Vec<_> = actual.params().collect();
        ensure!(
            expected.params.len() == params.len(),
            "parameter count mismatch"
        );
        for (expected, (_, actual)) in expected.params.iter().zip(params.iter()) {
            self.ty(&expected.ty, actual, 0)?;
        }
        let results: Vec<_> = actual.results().collect();
        ensure!(
            results.len() == usize::from(expected.result.is_some()),
            "result count mismatch"
        );
        if let Some(expected) = &expected.result {
            self.ty(expected, &results[0], 0)?;
        }
        Ok(())
    }

    fn resource(&mut self, key: &str, actual: ResourceType) -> Result<()> {
        ensure!(
            self.resources
                .iter()
                .all(|(other, ty)| other == key || *ty != actual),
            "distinct WIT resources share a canonical identity"
        );
        if let Some(previous) = self.resources.insert(key.to_string(), actual) {
            ensure!(previous == actual, "resource identity mismatch for {key}");
        }
        Ok(())
    }

    fn ty(&mut self, expected: &Wit, actual: &Type, depth: usize) -> Result<()> {
        ensure!(depth < 128, "WIT contract nesting exceeds 128");
        let depth = depth + 1;
        match (expected, actual) {
            (Wit::Ref(key), _) => {
                let definition = self
                    .world
                    .types
                    .get(key)
                    .with_context(|| format!("unknown WIT type {key}"))?;
                self.ty(&definition.ty, actual, depth)?;
            }
            (Wit::Bool, Type::Bool)
            | (Wit::U8, Type::U8)
            | (Wit::U16, Type::U16)
            | (Wit::U32, Type::U32)
            | (Wit::U64, Type::U64)
            | (Wit::S8, Type::S8)
            | (Wit::S16, Type::S16)
            | (Wit::S32, Type::S32)
            | (Wit::S64, Type::S64)
            | (Wit::F32, Type::Float32)
            | (Wit::F64, Type::Float64)
            | (Wit::Char, Type::Char)
            | (Wit::String, Type::String) => {}
            (Wit::Own(key), Type::Own(resource)) | (Wit::Borrow(key), Type::Borrow(resource)) => {
                self.resource(key, *resource)?
            }
            (Wit::List(expected), Type::List(actual)) => self.ty(expected, &actual.ty(), depth)?,
            (Wit::Future(expected), Type::Future(actual)) => {
                self.optional(expected.as_deref(), actual.ty().as_ref(), depth)?
            }
            (Wit::Stream(expected), Type::Stream(actual)) => {
                self.optional(expected.as_deref(), actual.ty().as_ref(), depth)?
            }
            (Wit::Option(expected), Type::Option(actual)) => {
                self.ty(expected, &actual.ty(), depth)?
            }
            (Wit::Tuple(expected), Type::Tuple(actual)) => {
                let actual: Vec<_> = actual.types().collect();
                ensure!(expected.len() == actual.len(), "tuple arity mismatch");
                for (e, a) in expected.iter().zip(&actual) {
                    self.ty(e, a, depth)?;
                }
            }
            (Wit::Record(expected), Type::Record(actual)) => {
                let actual: Vec<_> = actual.fields().collect();
                ensure!(
                    expected.len() == actual.len(),
                    "record field count mismatch"
                );
                for (e, a) in expected.iter().zip(&actual) {
                    ensure!(e.name == a.name, "record field name/order mismatch");
                    self.ty(&e.ty, &a.ty, depth)?;
                }
            }
            (Wit::Variant(expected), Type::Variant(actual)) => {
                let actual: Vec<_> = actual.cases().collect();
                ensure!(
                    expected.len() == actual.len(),
                    "variant case count mismatch"
                );
                for (e, a) in expected.iter().zip(&actual) {
                    ensure!(e.name == a.name, "variant case name/order mismatch");
                    self.optional(e.ty.as_ref(), a.ty.as_ref(), depth)?;
                }
            }
            (Wit::Result { ok, err }, Type::Result(actual)) => {
                self.optional(ok.as_deref(), actual.ok().as_ref(), depth)?;
                self.optional(err.as_deref(), actual.err().as_ref(), depth)?;
            }
            (Wit::Flags(expected), Type::Flags(actual)) => ensure!(
                expected.iter().map(String::as_str).eq(actual.names()),
                "flag names/order mismatch"
            ),
            (Wit::Enum(expected), Type::Enum(actual)) => ensure!(
                expected.iter().map(String::as_str).eq(actual.names()),
                "enum names/order mismatch"
            ),
            _ => bail!("WIT type mismatch: expected {expected:?}, component has {actual:?}"),
        }
        Ok(())
    }

    fn optional(
        &mut self,
        expected: Option<&Wit>,
        actual: Option<&Type>,
        depth: usize,
    ) -> Result<()> {
        match (expected, actual) {
            (None, None) => Ok(()),
            (Some(e), Some(a)) => self.ty(e, a, depth),
            _ => bail!("optional payload mismatch"),
        }
    }
}
