//! A sibling facade preserves synchronous signatures while kicking a private
//! asynchronous actor. Every payload layout still comes from wit-bindgen-c.
use super::*;
use anyhow::Context;
use serde::{Deserialize, Serialize};
#[cfg(all(test, target_os = "linux"))]
mod memory_tests;
#[cfg(test)]
mod tests;
mod types;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Forward {
    pub interface: String,
    pub function: String,
    pub export: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Plan {
    pub schema_version: u32,
    pub actor: String,
    pub signal: String,
    pub forwards: Vec<Forward>,
    pub imports: Vec<Binding>,
    pub forward_interface: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Binding {
    pub local: String,
    pub original: String,
}

pub(super) fn generate(
    resolve: &Resolve,
    world: WorldId,
    public: &crate::World,
    anchor: &str,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut resolve = resolve.clone();
    let original = resolve.worlds[world].clone();
    let mut facade = original.clone();
    facade.exports.clear();
    facade
        .imports
        .retain(|_, item| !matches!(item, WorldItem::Function(_)));
    // Types imported by the original remain shared imports. Public interfaces
    // become imports here, so their exported resources retain their identity.
    let sources = original
        .exports
        .iter()
        .map(|(key, item)| (key.clone(), resolve.name_world_key(key), item.clone()))
        .collect::<Vec<_>>();
    // Give exported source interfaces private canonical names in this clone.
    // This keeps distinct input/output resource interfaces from colliding when
    // the private forwarding interface imports both of them.
    for (_, _, item) in &sources {
        if let WorldItem::Interface { id, .. } = item {
            if resolve.interfaces[*id].clone_of.is_none() {
                continue;
            }
            let old = resolve.interfaces[*id].name.clone();
            let package = resolve.interfaces[*id].package.unwrap();
            if let Some(old) = old {
                if resolve.packages[package].interfaces.get(&old) == Some(id) {
                    resolve.packages[package].interfaces.shift_remove(&old);
                }
            }
            let name = format!("perry-producer-source-{}", id.index());
            resolve.packages[package]
                .interfaces
                .insert(name.clone(), *id);
            resolve.interfaces[*id].name = Some(name);
            resolve.interfaces[*id].clone_of = None;
        }
    }
    let mut imports = Vec::new();
    let mut renamed = BTreeMap::new();
    for (_, original_name, item) in &sources {
        if original_name == anchor {
            continue;
        }
        let key = if let WorldItem::Interface { id, .. } = item {
            WorldKey::Interface(*id)
        } else {
            let mut n = imports.len();
            loop {
                let key = WorldKey::Name(format!("perry-source-{n}"));
                if !facade.imports.contains_key(&key) {
                    break key;
                }
                n += 1;
            }
        };
        let local = resolve.name_world_key(&key);
        let mut item = item.clone();
        if let WorldItem::Function(function) = &mut item {
            function.name = local.clone();
        }
        renamed.insert(original_name.clone(), key.clone());
        facade.imports.insert(key, item);
        imports.push(Binding {
            local,
            original: original_name.clone(),
        });
    }
    let forwarding = resolve.interfaces.alloc(wit_parser::Interface {
        name: None,
        types: Default::default(),
        functions: Default::default(),
        docs: Default::default(),
        stability: Default::default(),
        package: original.package,
        span: Default::default(),
        clone_of: None,
    });
    let forward_interface = "perry-producer-forward".to_string();
    let forward_key = WorldKey::Name(forward_interface.clone());
    let mut mapped = BTreeMap::new();
    let mut root_forwards = std::collections::HashSet::new();
    let actor = asynchronous::actor_name(anchor);
    let mut entries = Vec::new();
    for interface in &public.exports {
        let source_interface = sources.iter().find(|(_, name, _)| name == &interface.name);
        for function in interface.functions.iter().filter(|f| !f.async_) {
            let (key, source) = if interface.name.is_empty() {
                let local = &renamed[&function.name];
                let WorldItem::Function(source) = &facade.imports[local] else {
                    unreachable!()
                };
                (None, source.clone())
            } else {
                let (_, _, item) = source_interface.expect("public interface");
                let WorldItem::Interface { id, .. } = item else {
                    unreachable!()
                };
                (
                    Some(renamed[&interface.name].clone()),
                    resolve.interfaces[*id].functions[&function.name].clone(),
                )
            };
            let export = format!("perry-forward-{}", entries.len());
            let mut forwarded = source.clone();
            forwarded.name = export.clone();
            forwarded.kind = wit_parser::FunctionKind::Freestanding;
            if interface.name.is_empty() {
                // World functions already use elaborated world types. Keep
                // those imported equality identities through the facade.
                let key = WorldKey::Name(export.clone());
                root_forwards.insert(key.clone());
                ensure!(
                    facade
                        .imports
                        .insert(key, WorldItem::Function(forwarded.clone()))
                        .is_none(),
                    "Facade forward name collision"
                );
            } else {
                for param in &mut forwarded.params {
                    param.ty = types::remap(&mut resolve, forwarding, &mut mapped, param.ty)?;
                }
                forwarded.result = forwarded
                    .result
                    .map(|ty| types::remap(&mut resolve, forwarding, &mut mapped, ty))
                    .transpose()?;
                resolve.interfaces[forwarding]
                    .functions
                    .insert(export.clone(), forwarded.clone());
            }
            entries.push((
                Forward {
                    interface: interface.name.clone(),
                    function: function.name.clone(),
                    export,
                },
                key,
                source,
                forwarded,
            ));
        }
    }
    // Keep the selected world ID and package in this private Resolve clone.
    // Root types belong to that world, and metadata encoding requires both
    // their owner and the package's world mapping to remain valid.
    facade.imports.insert(
        forward_key.clone(),
        WorldItem::Interface {
            id: forwarding,
            stability: Default::default(),
            docs: Default::default(),
            span: Default::default(),
            external_id: None,
        },
    );
    resolve.worlds[world] = facade;
    resolve.exportize(
        world,
        Some(original.name.clone()),
        Some(&|key, _| key == &forward_key || root_forwards.contains(key)),
    )?;
    let facade_id = world;
    let mut files = Files::default();
    wit_bindgen_c::Opts {
        no_sig_flattening: true,
        ..Default::default()
    }
    .build()
    .generate(&mut resolve, facade_id, &mut files)?;
    let world_name = resolve.worlds[facade_id].name.to_snake_case();
    // Reuse the same maintained type-name visitation as the JS adapter.
    let mut generator = Generator {
        resolve: &resolve,
        id: facade_id,
        world: public,
        names: BTreeMap::new(),
        resources: BTreeMap::new(),
        endpoints: BTreeMap::new(),
        endpoint_operations: BTreeMap::new(),
        resource_operations: BTreeMap::new(),
        source: String::new(),
        imports: Vec::new(),
        anchor: None,
    };
    for (key, item) in &resolve.worlds[facade_id].imports {
        if let WorldItem::Interface { id, .. } = item {
            let mut live = LiveTypes::default();
            live.add_interface(&resolve, *id);
            generator.define(live, Some((*id, key)), true);
        }
    }
    let mut live = LiveTypes::default();
    for item in resolve.worlds[facade_id].imports.values() {
        match item {
            WorldItem::Type { id, .. } => live.add_type_id(&resolve, *id),
            WorldItem::Function(f) => live.add_func(&resolve, f),
            WorldItem::Interface { .. } => {}
        }
    }
    generator.define(live, None, true);
    let mut live = LiveTypes::default();
    live.add_interface(&resolve, forwarding);
    generator.define(live, Some((forwarding, &forward_key)), false);
    let mut emitter = values::Emitter::new(
        &resolve,
        &generator.names,
        &generator.resources,
        &generator.endpoints,
        &world_name,
    );
    let mut source = String::from("#include \"perry_producer_facade.h\"\n#include <string.h>\n");
    let WorldItem::Function(actor_function) = &resolve.worlds[facade_id].imports[&renamed[&actor]]
    else {
        unreachable!()
    };
    let kick = wit_bindgen_c::c_func_name(
        true,
        &resolve,
        &world_name,
        None,
        actor_function,
        &HashMap::new(),
    );
    let upper = world_name.to_uppercase();
    for (forward, key, original, forwarded) in &entries {
        let name = wit_bindgen_c::c_func_name(
            false,
            &resolve,
            &world_name,
            if forward.interface.is_empty() {
                None
            } else {
                Some(&forward_key)
            },
            forwarded,
            &HashMap::new(),
        );
        let target = wit_bindgen_c::c_func_name(
            true,
            &resolve,
            &world_name,
            key.as_ref(),
            original,
            &HashMap::new(),
        );
        let indirect = forwarded
            .result
            .is_some_and(|ty| wit_bindgen_c::is_arg_by_pointer(&resolve, &ty));
        let result = forwarded.result.map(|ty| emitter.ctype(ty));
        let mut params = Vec::new();
        let mut args = Vec::new();
        let mut conversion = String::new();
        for (n, (param, original)) in forwarded.params.iter().zip(&original.params).enumerate() {
            let pointer = wit_bindgen_c::is_arg_by_pointer(&resolve, &param.ty);
            let ty = emitter.ctype(param.ty);
            let native = emitter.ctype(original.ty);
            params.push(format!("{ty} {}arg{n}", if pointer { "*" } else { "" }));
            writeln!(conversion, "_Static_assert(sizeof({ty}) == sizeof({native}) && _Alignof({ty}) == _Alignof({native}), \"Facade ABI mismatch\");\n{native} native{n};\nmemcpy(&native{n}, {}arg{n}, sizeof(native{n}));", if pointer { "" } else { "&" })?;
            args.push(format!("{}native{n}", if pointer { "&" } else { "" }));
        }
        if indirect {
            params.push(format!("{} *result", result.as_ref().unwrap()));
        }
        if params.is_empty() {
            params.push("void".into());
        }
        let ret = if indirect {
            "void"
        } else {
            result.as_deref().unwrap_or("void")
        };
        writeln!(
            source,
            "{ret} {name}({}) {{\n{conversion}",
            params.join(", ")
        )?;
        let native_result = original.result.map(|ty| emitter.ctype(ty));
        if let Some(ty) = &native_result {
            writeln!(source, "{ty} native_result;")?;
            if indirect {
                args.push("&native_result".into());
            }
        }
        if result.is_some() && !indirect {
            writeln!(source, "native_result = {target}({});", args.join(", "))?;
        } else {
            writeln!(source, "{target}({});", args.join(", "))?;
        }
        for (n, param) in original.params.iter().enumerate() {
            emitter.free(param.ty, &format!("&native{n}"));
        }
        source.push_str(&std::mem::take(&mut emitter.source));
        writeln!(source, "{world_name}_subtask_status_t status = {kick}();\nif ({upper}_SUBTASK_STATE(status) != {upper}_SUBTASK_RETURNED) __builtin_trap();\n{world_name}_subtask_t task = {upper}_SUBTASK_HANDLE(status);\nif (task) {world_name}_subtask_drop(task);")?;
        if let Some(ty) = &result {
            let native = native_result.as_ref().unwrap();
            writeln!(source, "_Static_assert(sizeof({ty}) == sizeof({native}) && _Alignof({ty}) == _Alignof({native}), \"Facade result ABI mismatch\");")?;
            if indirect {
                source.push_str("memcpy(result, &native_result, sizeof(native_result));\n");
            } else {
                writeln!(source, "{ty} result;\nmemcpy(&result, &native_result, sizeof(result));\nreturn result;")?;
            }
        }
        source.push_str("}\n");
    }
    // Producer and facade use separate core modules. Their SDK symbol names
    // may coincide, but their files share one output directory.
    let mut output = BTreeMap::new();
    for (name, bytes) in files.iter() {
        let suffix = name
            .strip_prefix(&world_name)
            .context("Unexpected facade backend file")?;
        let name = format!("perry_producer_facade{suffix}");
        let bytes = if name.ends_with(".c") {
            String::from_utf8(bytes.to_vec())?
                .replace(
                    &format!("\"{world_name}.h\""),
                    "\"perry_producer_facade.h\"",
                )
                .into_bytes()
        } else {
            bytes.to_vec()
        };
        output.insert(name, bytes);
    }
    output.insert("perry_producer_forward.c".into(), source.into_bytes());
    let plan = Plan {
        schema_version: 1,
        actor,
        signal: anchor.into(),
        forwards: entries
            .into_iter()
            .map(|(forward, _, _, _)| forward)
            .collect(),
        imports,
        forward_interface,
    };
    output.insert(
        "perry_producer_facade.json".into(),
        serde_json::to_vec_pretty(&plan)?,
    );
    Ok(output)
}
