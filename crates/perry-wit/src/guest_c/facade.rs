//! A sibling facade preserves synchronous signatures while kicking a private
//! asynchronous actor. Every payload layout still comes from wit-bindgen-c.
use super::*;
use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

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
    facade.name = "perry-producer-facade".into();
    facade.package = None;
    facade.exports.clear();
    facade
        .imports
        .retain(|_, item| !matches!(item, WorldItem::Function(_)));
    // Types imported by the original remain shared imports. Public interfaces
    // become imports here, so their exported resources retain their identity.
    let mut imports = Vec::new();
    let mut renamed = BTreeMap::new();
    for (key, item) in &original.exports {
        if resolve.name_world_key(key) != anchor {
            let mut n = imports.len();
            let local = loop {
                let local = format!("perry-source-{n}");
                if !facade.imports.contains_key(&WorldKey::Name(local.clone())) {
                    break local;
                }
                n += 1;
            };
            let mut item = item.clone();
            if let WorldItem::Function(function) = &mut item {
                function.name = local.clone();
            }
            let original = resolve.name_world_key(key);
            renamed.insert(original.clone(), WorldKey::Name(local.clone()));
            facade.imports.insert(WorldKey::Name(local.clone()), item);
            imports.push(Binding { local, original });
        }
    }
    let actor = asynchronous::actor_name(anchor);
    let mut entries = Vec::new();
    for interface in &public.exports {
        let key = original
            .exports
            .keys()
            .find(|key| resolve.name_world_key(key) == interface.name);
        for function in interface.functions.iter().filter(|f| !f.async_) {
            let (key, source) = if interface.name.is_empty() {
                let local = &renamed[&function.name];
                let WorldItem::Function(source) = &facade.imports[local] else {
                    unreachable!()
                };
                (None, source.clone())
            } else {
                let key = key.expect("public interface");
                let WorldItem::Interface { id, .. } = &original.exports[key] else {
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
            facade.exports.insert(
                WorldKey::Name(export.clone()),
                WorldItem::Function(forwarded.clone()),
            );
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
    let facade_id = resolve.worlds.alloc(facade);
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
    live.add_world(&resolve, facade_id);
    generator.define(live, None, true);
    let emitter = values::Emitter::new(
        &resolve,
        &generator.names,
        &generator.resources,
        &generator.endpoints,
        &world_name,
    );
    let mut source = format!("#include \"{world_name}.h\"\n");
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
    for (_, key, original, forwarded) in &entries {
        let name = wit_bindgen_c::c_func_name(
            false,
            &resolve,
            &world_name,
            None,
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
        for (n, param) in forwarded.params.iter().enumerate() {
            let pointer = if wit_bindgen_c::is_arg_by_pointer(&resolve, &param.ty) {
                "*"
            } else {
                ""
            };
            params.push(format!("{} {pointer}arg{n}", emitter.ctype(param.ty)));
            args.push(format!("arg{n}"));
        }
        if indirect {
            params.push(format!("{} *result", result.as_ref().unwrap()));
            args.push("result".into());
        }
        if params.is_empty() {
            params.push("void".into());
        }
        let ret = if indirect {
            "void"
        } else {
            result.as_deref().unwrap_or("void")
        };
        writeln!(source, "{ret} {name}({}) {{", params.join(", "))?;
        if result.is_some() && !indirect {
            writeln!(source, "{ret} result = {target}({});", args.join(", "))?;
        } else {
            writeln!(source, "{target}({});", args.join(", "))?;
        }
        writeln!(source, "{world_name}_subtask_status_t status = {kick}();\nif ({upper}_SUBTASK_STATE(status) != {upper}_SUBTASK_RETURNED) __builtin_trap();\n{world_name}_subtask_t task = {upper}_SUBTASK_HANDLE(status);\nif (task) {world_name}_subtask_drop(task);")?;
        if result.is_some() && !indirect {
            source.push_str("return result;\n");
        }
        source.push_str("}\n");
    }
    let mut output: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .collect();
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
    };
    output.insert(
        "perry_producer_facade.json".into(),
        serde_json::to_vec_pretty(&plan)?,
    );
    Ok(output)
}
