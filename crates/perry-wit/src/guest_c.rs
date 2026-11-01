//! The maintained C backend owns the canonical ABI. This module connects its
//! typed C functions to Perry's rooted frame API; it never invents Wasm layouts.
#[cfg(test)]
mod agent_tests;
mod asynchronous;
mod endpoints;
mod values;

use anyhow::{ensure, Result};
use heck::ToSnakeCase;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    path::Path,
};
use wit_bindgen_core::Files;
use wit_parser::{
    Function, InterfaceId, LiveTypes, Resolve, TypeDefKind, TypeId, WorldId, WorldItem, WorldKey,
};

type Names = BTreeMap<TypeId, String>;

pub fn canonical_guest(path: &Path, selected: Option<&str>) -> Result<BTreeMap<String, Vec<u8>>> {
    generate_guest(path, selected, false)
}

/// Experimental async generation, including the Linux-qualified resource
/// ownership/cancellation and guest endpoint read/write profiles. Typed
/// factories and post-return writers behind async exports are qualified on
/// Linux; synchronous background producer scheduling remains to implement.
/// Public --guest-c async output and future/stream values remain guarded.
#[doc(hidden)]
pub fn experimental_async_guest(
    path: &Path,
    selected: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    generate_guest(path, selected, true)
}

fn generate_guest(
    path: &Path,
    selected: Option<&str>,
    experimental: bool,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(path)?;
    let id = resolve.select_world(&[package], selected)?;
    // C generation nominalizes the graph. Do this before the manifest too,
    // so its resource factories and the generated callbacks share TypeIds.
    resolve.generate_nominal_type_ids(id);
    let world = crate::resolve::convert(&resolve, id)?;
    ensure!(
        experimental
            || world
                .types
                .values()
                .all(|d| !matches!(d.ty, crate::Type::Future(_) | crate::Type::Stream(_))),
        "future/stream guest values require the P3 rooted-frame adapter"
    );
    ensure!(
        experimental
            || world
                .imports
                .iter()
                .chain(&world.exports)
                .flat_map(|i| &i.functions)
                .all(|f| !f.async_),
        "async guest functions require the P3 rooted-frame adapter"
    );
    let anchor = if experimental {
        Some(asynchronous::add_wake_anchor(&mut resolve, id)?)
    } else {
        None
    };
    generate_resolved(resolve, id, world, anchor)
}

fn generate_resolved(
    mut resolve: Resolve,
    id: WorldId,
    world: crate::World,
    anchor: Option<String>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = Files::default();
    wit_bindgen_c::Opts {
        no_sig_flattening: true,
        autodrop_borrows: if anchor.is_some() {
            wit_bindgen_c::Enabled::No
        } else {
            wit_bindgen_c::Enabled::Yes
        },
        ..Default::default()
    }
    .build()
    .generate(&mut resolve, id, &mut files)?;
    let snake = resolve.worlds[id].name.to_snake_case();
    let mut output: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .collect();
    let mut generator = Generator {
        resolve: &resolve,
        id,
        world: &world,
        names: BTreeMap::new(),
        resources: BTreeMap::new(),
        endpoints: BTreeMap::new(),
        endpoint_operations: BTreeMap::new(),
        resource_operations: BTreeMap::new(),
        source: format!("#include \"{snake}.h\"\n#include \"perry_component_frame.h\"\n"),
        imports: Vec::new(),
        anchor,
    };
    generator.generate()?;
    output.insert("perry_adapter.c".into(), generator.source.into_bytes());
    output.insert(
        "perry_component_frame.h".into(),
        include_bytes!("guest_frame.h").to_vec(),
    );
    output.insert(
        "perry_component_agent.h".into(),
        include_bytes!("guest_agent.h").to_vec(),
    );
    Ok(output)
}

#[derive(Clone)]
struct Resource {
    id: u32,
    own_name: String,
    borrow_name: String,
    name: String,
    local: bool,
}
type Resources = BTreeMap<TypeId, Resource>;
type Endpoints = BTreeMap<TypeId, u32>;

struct Generator<'a> {
    resolve: &'a Resolve,
    id: WorldId,
    world: &'a crate::World,
    names: Names,
    resources: Resources,
    endpoints: Endpoints,
    endpoint_operations: BTreeMap<u32, String>,
    resource_operations: BTreeMap<u32, Resource>,
    source: String,
    imports: Vec<(usize, usize)>,
    anchor: Option<String>,
}

impl Generator<'_> {
    fn define(
        &mut self,
        live: LiveTypes,
        interface: Option<(InterfaceId, &WorldKey)>,
        import: bool,
    ) {
        for id in live.iter() {
            if self.names.contains_key(&id) {
                continue;
            }
            let (info, encoded) = wit_bindgen_c::gen_type_name(self.resolve, id);
            let namespace = match info {
                wit_bindgen_c::CTypeNameInfo::Anonymous { is_prim: true } => {
                    self.resolve.worlds[self.id].name.to_snake_case()
                }
                _ => wit_bindgen_c::owner_namespace(
                    interface,
                    import,
                    self.resolve.worlds[self.id].name.clone(),
                    self.resolve,
                    id,
                    &HashMap::new(),
                ),
            };
            let name = format!("{namespace}_{encoded}_t");
            if matches!(
                self.resolve.types[id].kind,
                TypeDefKind::Future(_) | TypeDefKind::Stream(_)
            ) {
                // Root exports, like root imports, use the manifest's tN
                // namespace. Only exported interfaces introduce eN types.
                let exported = !import && interface.is_some();
                let ty = u32::try_from(id.index() * 2 + usize::from(exported))
                    .expect("async type index");
                self.endpoints.insert(id, ty);
            }
            if matches!(self.resolve.types[id].kind, TypeDefKind::Resource) {
                let resource = Resource {
                    id: u32::try_from(id.index() * 2 + usize::from(!import))
                        .expect("resource type index"),
                    own_name: format!("{namespace}_own_{encoded}_t"),
                    borrow_name: format!("{namespace}_borrow_{encoded}_t"),
                    name: name.clone(),
                    local: !import,
                };
                self.resource_operations
                    .insert(resource.id, resource.clone());
                self.resources.insert(id, resource);
            }
            self.names.insert(id, name);
        }
    }

    fn generate(&mut self) -> Result<()> {
        self.source.push_str(
            "static void *perry_initialize(void);\nstatic void perry_endpoint_flush(void);\n",
        );
        if self.anchor.is_some() {
            self.source.push_str("/* PERRY_ASYNC_SCHEDULER */\n");
        }
        let world = &self.resolve.worlds[self.id];
        // Match WorldGenerator's visitation order, including the separate
        // namespaces when an interface is both imported and exported.
        for (key, item) in &world.imports {
            if let WorldItem::Interface { id, .. } = item {
                let mut live = LiveTypes::default();
                live.add_interface(self.resolve, *id);
                self.define(live, Some((*id, key)), true);
                for function in self.resolve.interfaces[*id].functions.values() {
                    self.function(Some(key), function, true)?;
                }
            }
        }
        let mut live = LiveTypes::default();
        for item in world.imports.values() {
            if let WorldItem::Type { id, .. } = item {
                live.add_type_id(self.resolve, *id);
            }
        }
        self.define(live, None, true);
        for import in [true, false] {
            let items = if import {
                &world.imports
            } else {
                &world.exports
            };
            let mut live = LiveTypes::default();
            for item in items.values() {
                if let WorldItem::Function(function) = item {
                    live.add_func(self.resolve, function);
                }
            }
            self.define(live, None, import);
            if !import {
                if let Some(anchor) = &self.anchor {
                    let WorldItem::Function(function) =
                        &world.exports[&WorldKey::Name(anchor.clone())]
                    else {
                        unreachable!()
                    };
                    let Some(wit_parser::Type::Id(signal)) = function.result else {
                        unreachable!()
                    };
                    // Resolve the signal through the same C-name visitation as
                    // every other value; it may belong to an imported interface.
                    let signal = self.names[&signal].strip_suffix("_t").unwrap();
                    self.source = self.source.replacen(
                        "/* PERRY_ASYNC_SCHEDULER */\n",
                        &asynchronous::scheduler(&world.name.to_snake_case(), anchor, signal),
                        1,
                    );
                }
            }
            for item in items.values() {
                if let WorldItem::Function(function) = item {
                    if !import
                        && self.anchor.as_ref().is_some_and(|anchor| {
                            *anchor == function.name
                                || asynchronous::actor_name(anchor) == function.name
                        })
                    {
                        continue;
                    }
                    self.function(None, function, import)?;
                }
            }
        }
        let retained =
            wit_bindgen_c::imported_types_used_by_exported_interfaces(self.resolve, self.id);
        self.names.retain(|id, _| retained.contains(id));
        self.resources.retain(|id, _| retained.contains(id));
        self.endpoints.retain(|id, _| retained.contains(id));
        for (key, item) in &world.exports {
            if let WorldItem::Interface { id, .. } = item {
                let mut live = LiveTypes::default();
                live.add_interface(self.resolve, *id);
                self.define(live, Some((*id, key)), false);
                for function in self.resolve.interfaces[*id].functions.values() {
                    self.function(Some(key), function, false)?;
                }
            }
        }
        self.resource_adapters()?;
        self.endpoint_adapters()?;
        self.source.push_str("static uint32_t perry_import(void *f, uint32_t interface, uint32_t function, uint32_t args) {\n");
        for (interface, function) in &self.imports {
            writeln!(self.source, "if (interface == {interface} && function == {function}) return perry_import_{interface}_{function}(f, args);")?;
        }
        self.source
            .push_str("perry_bad(f, \"Unknown canonical import\"); return 0;\n}\n");
        let contract = serde_json::to_vec(self.world)?;
        writeln!(
            self.source,
            "static const uint8_t perry_contract[] = {{{}}};",
            contract
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )?;
        self.source.push_str("static const perry_guest_ops perry_ops = {2, sizeof(perry_guest_ops), perry_contract, sizeof(perry_contract), perry_import, perry_resource};\n");
        if !self.endpoint_operations.is_empty() {
            self.source.push_str("static void perry_endpoint_install(void) { if (!js_component_set_guest_endpoint_ops(&perry_endpoint_ops) || !js_component_set_guest_producer_ops(&perry_producer_ops)) __builtin_trap(); }\n");
        } else {
            self.source
                .push_str("static void perry_endpoint_install(void) {}\n");
        }
        self.source.push_str(
            r#"
extern void perry_module_init(void);
static unsigned perry_initialized;
static void perry_module_start(void *data) { (void)data; perry_module_init(); }
static void *perry_initialize(void) {
    if (perry_initialized == 2) return perry_component_frame_new();
    if (perry_initialized != 0) {
        void *f = perry_component_frame_new();
        perry_bad(f, "Component initialization failed or reentered");
        return f;
    }
    perry_initialized = 1;
    if (!js_component_set_guest_ops(&perry_ops)) __builtin_trap();
    perry_endpoint_install();
    void *f = perry_component_frame_bootstrap((const void *)perry_module_start, NULL);
    if (f == NULL) __builtin_trap();
    perry_initialized = perry_component_frame_failed(f) ? 3 : 2;
    return f;
}
"#,
        );
        Ok(())
    }

    fn resource_adapters(&mut self) -> Result<()> {
        for (id, resource) in &self.resource_operations {
            if !resource.local {
                continue;
            }
            let stem = resource.name.strip_suffix("_t").unwrap();
            let ty = *id;
            // wit-bindgen uses a pointer-shaped rep, but the canonical ABI
            // permits an opaque i32. Never allocate or dereference a JS address.
            writeln!(self.source, "void {stem}_destructor({} *rep) {{\nvoid *f = perry_component_frame_new();\nperry_component_frame_resource_destroy(f, {ty}, (uint32_t)(uintptr_t)rep);\nbool failed = perry_component_frame_failed(f);\nperry_component_frame_drop(f);\nif (failed) __builtin_trap();\n}}", resource.name)?;
        }
        self.source.push_str("static uint32_t perry_resource(void *f, uint32_t type, uint32_t operation, uint32_t value) {\nswitch (type) {\n");
        for (id, resource) in &self.resource_operations {
            let stem = resource.name.strip_suffix("_t").unwrap();
            let own = &resource.own_name;
            writeln!(self.source, "case {}:\nif (operation == 1) {{ {stem}_drop_own(({own}){{ (int32_t)value }}); return 0; }}", id)?;
            if !resource.local && self.anchor.is_some() {
                let borrow = &resource.borrow_name;
                writeln!(self.source, "if (operation == 2) {{ {stem}_drop_borrow(({borrow}){{ (int32_t)value }}); return 0; }}")?;
            }
            if resource.local {
                writeln!(self.source, "if (operation == 0) return (uint32_t){stem}_new(({} *)(uintptr_t)value).__handle;", resource.name)?;
            }
            self.source.push_str("break;\n");
        }
        self.source
            .push_str("}\nperry_bad(f, \"Unknown guest resource operation\"); return 0;\n}\n");
        Ok(())
    }

    fn function(
        &mut self,
        key: Option<&WorldKey>,
        function: &Function,
        import: bool,
    ) -> Result<()> {
        // Interfaces can declare unused endpoint types for which the maintained
        // backend emits no canonical helpers. Only reachable function types
        // belong in the dispatcher; the private wake stream isn't a JS value.
        let mut live = LiveTypes::default();
        live.add_func(self.resolve, function);
        for id in live.iter() {
            if let Some(ty) = self.endpoints.get(&id).copied() {
                if !self.endpoint_operations.contains_key(&ty) {
                    let name = self.names[&id].clone();
                    // Encode the payload while this import/export namespace
                    // is active. Exported interfaces deliberately prune and
                    // rebuild these maps; readers retain the emitted C code.
                    self.reader_adapter(ty, id, &name)?;
                    self.writer_adapter(ty, id, &name)?;
                    self.endpoint_operations.insert(ty, name);
                }
            }
        }
        let name = key
            .map(|key| self.resolve.name_world_key(key))
            .unwrap_or_default();
        let interfaces = if import {
            &self.world.imports
        } else {
            &self.world.exports
        };
        let i = interfaces
            .iter()
            .position(|item| item.name == name)
            .unwrap();
        let f = interfaces[i]
            .functions
            .iter()
            .position(|item| item.name == function.name)
            .unwrap();
        let snake = self.resolve.worlds[self.id].name.to_snake_case();
        let mut emitter = values::Emitter::new(
            self.resolve,
            &self.names,
            &self.resources,
            &self.endpoints,
            &snake,
        );
        let cname = wit_bindgen_c::c_func_name(
            import,
            self.resolve,
            &snake,
            key,
            function,
            &HashMap::new(),
        );
        if !import && self.anchor.is_some() {
            return self.experimental_export(function, i, f, &cname);
        }
        if import && function.kind.is_async() {
            self.imports.push((i, f));
            return self.experimental_import(function, i, f, &cname);
        }
        let indirect = function
            .result
            .is_some_and(|ty| wit_bindgen_c::is_arg_by_pointer(self.resolve, &ty));
        if import {
            self.imports.push((i, f));
            writeln!(emitter.source, "static uint32_t perry_import_{i}_{f}(void *f, uint32_t args) {{\nperry_allocations arena = {{0}}; uint32_t output = 0;")?;
            for (n, param) in function.params.iter().enumerate() {
                writeln!(
                    emitter.source,
                    "{} arg{n} = {{0}};",
                    emitter.ctype(param.ty)
                )?;
                let slot = emitter.slot(&format!("perry_component_frame_array_get(f, args, {n})"));
                emitter.lower(param.ty, &slot, &format!("arg{n}"))?;
            }
            let mut args: Vec<_> = function
                .params
                .iter()
                .enumerate()
                .map(|(n, param)| {
                    format!(
                        "{}arg{n}",
                        if wit_bindgen_c::is_arg_by_pointer(self.resolve, &param.ty) {
                            "&"
                        } else {
                            ""
                        }
                    )
                })
                .collect();
            if let Some(ty) = function.result {
                writeln!(emitter.source, "{} result = {{0}};", emitter.ctype(ty))?;
                if indirect {
                    args.push("&result".into());
                }
            }
            emitter
                .source
                .push_str("perry_component_frame_resource_commit(f);\nif (!perry_component_frame_failed(f)) {\n");
            writeln!(
                emitter.source,
                "{}{cname}({});",
                if function.result.is_some() && !indirect {
                    "result = "
                } else {
                    ""
                },
                args.join(", ")
            )?;
            if let Some(ty) = function.result {
                let value = emitter.lift(ty, "result")?;
                writeln!(emitter.source, "output = {value};")?;
                emitter.free(ty, "&result");
            }
            emitter
                .source
                .push_str("}\nperry_allocations_end(&arena, false); return output;\n}\n");
        } else {
            let ret = function
                .result
                .filter(|_| !indirect)
                .map(|ty| emitter.ctype(ty))
                .unwrap_or_else(|| "void".into());
            let mut params: Vec<_> = function
                .params
                .iter()
                .enumerate()
                .map(|(n, param)| {
                    format!(
                        "{} {}arg{n}",
                        emitter.ctype(param.ty),
                        if wit_bindgen_c::is_arg_by_pointer(self.resolve, &param.ty) {
                            "*"
                        } else {
                            ""
                        }
                    )
                })
                .collect();
            if indirect {
                params.push(format!(
                    "{} *result",
                    emitter.ctype(function.result.unwrap())
                ));
            }
            if params.is_empty() {
                params.push("void".into());
            }
            writeln!(emitter.source, "{ret} {cname}({}) {{\nvoid *f = perry_initialize();\nperry_allocations arena = {{0}};", params.join(", "))?;
            let args = emitter.slot(&format!(
                "perry_component_frame_array(f, {})",
                function.params.len()
            ));
            for (n, param) in function.params.iter().enumerate() {
                let indirect = wit_bindgen_c::is_arg_by_pointer(self.resolve, &param.ty);
                let input = format!("{}arg{n}", if indirect { "*" } else { "" });
                let value = emitter.lift(param.ty, &input)?;
                writeln!(
                    emitter.source,
                    "perry_component_frame_array_set(f, {args}, {n}, {value});"
                )?;
                emitter.free(
                    param.ty,
                    &format!("{}arg{n}", if indirect { "" } else { "&" }),
                );
            }
            let value = emitter.slot(&format!("perry_component_frame_call(f, {i}, {f}, {args})"));
            writeln!(emitter.source, "(void){value};")?;
            if let Some(ty) = function.result {
                if indirect {
                    emitter
                        .source
                        .push_str("memset(result, 0, sizeof(*result));\n");
                } else {
                    writeln!(emitter.source, "{} result = {{0}};", emitter.ctype(ty))?;
                }
                emitter.lower(ty, &value, if indirect { "*result" } else { "result" })?;
            }
            emitter.source.push_str("perry_component_frame_resource_commit(f);\nbool failed = perry_component_frame_failed(f);\nperry_allocations_end(&arena, !failed);\nperry_component_frame_drop(f);\nif (failed) __builtin_trap();\n");
            if function.result.is_some() && !indirect {
                emitter.source.push_str("return result;\n");
            }
            emitter.source.push_str("}\n");
        }
        self.source.push_str(&emitter.source);
        Ok(())
    }
}

fn literal(text: &str) -> String {
    format!(
        "\"{}\"",
        text.as_bytes()
            .iter()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>()
    )
}
