//! The maintained C backend owns the canonical ABI. This module connects its
//! typed C functions to Perry's rooted frame API; it never invents Wasm layouts.
mod values;

use anyhow::{bail, Result};
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
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(path)?;
    let id = resolve.select_world(&[package], selected)?;
    let world = crate::resolve::convert(&resolve, id)?;
    // Fail before wit-bindgen's C backend reaches a type it cannot represent.
    let mut live = LiveTypes::default();
    live.add_world(&resolve, id);
    for ty in live.iter() {
        match &resolve.types[ty].kind {
            TypeDefKind::Resource | TypeDefKind::Handle(_) => {
                bail!("canonical Perry guest resource adapters are not implemented yet")
            }
            _ => {}
        }
    }
    let mut files = Files::default();
    wit_bindgen_c::Opts {
        no_sig_flattening: true,
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
        source: format!("#include \"{snake}.h\"\n#include \"perry_component_frame.h\"\n"),
        imports: Vec::new(),
    };
    generator.generate()?;
    output.insert("perry_adapter.c".into(), generator.source.into_bytes());
    output.insert(
        "perry_component_frame.h".into(),
        include_bytes!("guest_frame.h").to_vec(),
    );
    Ok(output)
}

struct Generator<'a> {
    resolve: &'a Resolve,
    id: WorldId,
    world: &'a crate::World,
    names: Names,
    source: String,
    imports: Vec<(usize, usize)>,
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
            self.names.insert(id, format!("{namespace}_{encoded}_t"));
        }
    }

    fn generate(&mut self) -> Result<()> {
        self.source
            .push_str("static void perry_initialize(void *f);\n");
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
            for item in items.values() {
                if let WorldItem::Function(function) = item {
                    self.function(None, function, import)?;
                }
            }
        }
        let retained =
            wit_bindgen_c::imported_types_used_by_exported_interfaces(self.resolve, self.id);
        self.names.retain(|id, _| retained.contains(id));
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
        self.source.push_str("static const perry_guest_ops perry_ops = {1, sizeof(perry_guest_ops), perry_contract, sizeof(perry_contract), perry_import};\n");
        self.source.push_str(r#"
extern void perry_module_init(void);
static unsigned perry_initialized;
static void perry_initialize(void *f) {
    if (perry_initialized == 2) return;
    if (perry_initialized != 0) { perry_bad(f, "Component initialization failed or reentered"); return; }
    perry_initialized = 1;
    if (!js_component_set_guest_ops(&perry_ops)) perry_bad(f, "Component adapter ABI mismatch");
    perry_component_frame_initialize(f, (const void *)perry_module_init);
    perry_initialized = perry_component_frame_failed(f) ? 3 : 2;
}
"#);
        Ok(())
    }

    fn function(
        &mut self,
        key: Option<&WorldKey>,
        function: &Function,
        import: bool,
    ) -> Result<()> {
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
        let mut emitter = values::Emitter::new(self.resolve, &self.names, &snake);
        let cname = wit_bindgen_c::c_func_name(
            import,
            self.resolve,
            &snake,
            key,
            function,
            &HashMap::new(),
        );
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
                .push_str("if (!perry_component_frame_failed(f)) {\n");
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
            writeln!(emitter.source, "{ret} {cname}({}) {{\nvoid *f = perry_component_frame_new();\nperry_allocations arena = {{0}};\nperry_initialize(f);", params.join(", "))?;
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
            emitter.source.push_str("bool failed = perry_component_frame_failed(f);\nperry_allocations_end(&arena, !failed);\nperry_component_frame_drop(f);\nif (failed) __builtin_trap();\n");
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
