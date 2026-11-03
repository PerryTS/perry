use crate::{Function, Interface, Type, World};
use std::fmt::Write;

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("string")
}

struct Typescript<'a> {
    world: &'a World,
}

impl Typescript<'_> {
    fn alias(&self, key: &str) -> String {
        let name = &self.world.types[key].name;
        let suffix: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("{key}_{suffix}")
    }

    fn ty(&self, ty: &Type) -> String {
        match ty {
            Type::Bool => "boolean".into(),
            Type::U8
            | Type::U16
            | Type::U32
            | Type::S8
            | Type::S16
            | Type::S32
            | Type::F32
            | Type::F64 => "number".into(),
            Type::U64 | Type::S64 => "bigint".into(),
            Type::Char | Type::String => "string".into(),
            Type::Ref(key) => self.alias(key),
            Type::Record(fields) => format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|f| format!("{}: {};", quoted(&f.name), self.ty(&f.ty)))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Type::Tuple(types) => format!(
                "[{}]",
                types
                    .iter()
                    .map(|t| self.ty(t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Flags(names) => format!("Array<{}>", union(names.iter().map(|n| quoted(n)))),
            Type::Enum(names) => union(names.iter().map(|n| quoted(n))),
            Type::Variant(cases) => union(
                cases
                    .iter()
                    .map(|case| tagged(&case.name, case.ty.as_ref().map(|t| self.ty(t)))),
            ),
            Type::Option(inner) => format!(
                "{} | {}",
                tagged("none", None),
                tagged("some", Some(self.ty(inner)))
            ),
            Type::Result { ok, err } => format!(
                "{} | {}",
                tagged("ok", ok.as_deref().map(|t| self.ty(t))),
                tagged("err", err.as_deref().map(|t| self.ty(t)))
            ),
            Type::List(inner) if self.is_byte(inner) => "Uint8Array".into(),
            Type::List(inner) => format!("Array<{}>", self.ty(inner)),
            Type::Future(inner) => format!(
                "ComponentFuture<{}>",
                inner
                    .as_deref()
                    .map_or_else(|| "void".into(), |t| self.ty(t))
            ),
            Type::Stream(inner) => {
                let payload = inner
                    .as_deref()
                    .map_or_else(|| "void".into(), |t| self.ty(t));
                if inner.as_deref().is_some_and(|t| self.is_byte(t)) {
                    format!("ComponentStream<{payload}, Uint8Array>")
                } else {
                    format!("ComponentStream<{payload}>")
                }
            }
            Type::Resource => "never".into(), // Resources are used through own/borrow handles.
            Type::Own(key) => format!(
                "OwnedResource<{}>",
                quoted(&format!("{}/{}", self.world.name, key))
            ),
            Type::Borrow(key) => format!(
                "BorrowedResource<{}>",
                quoted(&format!("{}/{}", self.world.name, key))
            ),
        }
    }

    fn is_resource<'b>(&'b self, mut ty: &'b Type) -> bool {
        for _ in 0..=self.world.types.len() {
            match ty {
                Type::Resource => return true,
                Type::Ref(key) => ty = &self.world.types[key].ty,
                _ => return false,
            }
        }
        false
    }

    fn is_byte<'b>(&'b self, mut ty: &'b Type) -> bool {
        // WIT aliases preserve canonical representation. Keep declarations in
        // agreement with the runtime, which resolves aliases before lifting.
        for _ in 0..=self.world.types.len() {
            match ty {
                Type::U8 => return true,
                Type::Ref(key) => match self.world.types.get(key) {
                    Some(definition) => ty = &definition.ty,
                    None => return false,
                },
                _ => return false,
            }
        }
        false
    }

    fn signature(&self, function: &Function) -> String {
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(i, field)| format!("arg{i}: {}", self.ty(&field.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("({params}) => {}", self.result(function))
    }

    fn result(&self, function: &Function) -> String {
        let result = function
            .result
            .as_ref()
            .map_or_else(|| "void".into(), |t| self.ty(t));
        if function.async_ {
            format!("Promise<{result}>")
        } else {
            result
        }
    }

    fn interface(&self, out: &mut String, name: &str, interfaces: &[Interface]) {
        writeln!(out, "export interface {name} {{").unwrap();
        for interface in interfaces {
            if !interface.name.is_empty() {
                writeln!(out, "  {}: {{", quoted(&interface.name)).unwrap();
            }
            for function in &interface.functions {
                writeln!(
                    out,
                    "    {}: {};",
                    quoted(&function.name),
                    self.signature(function)
                )
                .unwrap();
            }
            if !interface.name.is_empty() {
                writeln!(out, "  }};").unwrap();
            }
        }
        writeln!(out, "}}\n").unwrap();
    }

    fn wrappers(&self, interfaces: &[Interface], call: &str) -> String {
        let mut out = String::from("{\n");
        for interface in interfaces {
            if !interface.name.is_empty() {
                writeln!(out, "    {}: {{", quoted(&interface.name)).unwrap();
            }
            for function in &interface.functions {
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("arg{i}: {}", self.ty(&p.ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let args = (0..function.params.len())
                    .map(|i| format!("arg{i}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let result = self.result(function);
                writeln!(
                    out,
                    "      {}: ({params}): {result} => {call}{}, {}, [{args}]) as {result},",
                    quoted(&function.name),
                    quoted(&interface.name),
                    quoted(&function.name)
                )
                .unwrap();
            }
            if !interface.name.is_empty() {
                writeln!(out, "    }},").unwrap();
            }
        }
        out.push_str("  }");
        out
    }

    fn host_resources(&self, out: &mut String) {
        out.push_str("export interface HostResources {\n");
        for interface in &self.world.imports {
            if interface.resources.is_empty() {
                continue;
            }
            writeln!(out, "  {}: {{", quoted(&interface.name)).unwrap();
            for (name, key) in &interface.resources {
                let tag = quoted(&format!("{}/{key}", self.world.name));
                writeln!(out, "    {}: {{ create(payload: unknown, destructor?: (payload: unknown) => void): OwnedResource<{tag}>; value(resource: BorrowedResource<{tag}>): unknown; }};", quoted(name)).unwrap();
            }
            out.push_str("  };\n");
        }
        out.push_str(
            "}\n\nfunction hostResources(handle: ComponentHandle): HostResources {\n  return {\n",
        );
        for interface in &self.world.imports {
            if interface.resources.is_empty() {
                continue;
            }
            writeln!(out, "    {}: {{", quoted(&interface.name)).unwrap();
            for (name, key) in &interface.resources {
                let tag = quoted(&format!("{}/{key}", self.world.name));
                let key = quoted(key);
                writeln!(out, "      {}: {{\n        create: (payload: unknown, destructor?: (payload: unknown) => void): OwnedResource<{tag}> => createComponentHostResource(handle, {key}, payload, destructor),\n        value: (resource: BorrowedResource<{tag}>): unknown => getComponentHostResourceValue(handle, {key}, resource),\n      }},", quoted(name)).unwrap();
            }
            out.push_str("    },\n");
        }
        out.push_str("  };\n}\n\n");
    }
}

fn union(items: impl Iterator<Item = String>) -> String {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        "never".into()
    } else {
        items.join(" | ")
    }
}

fn tagged(tag: &str, value: Option<String>) -> String {
    match value {
        Some(value) => format!("{{ tag: {}; value: {value} }}", quoted(tag)),
        None => format!("{{ tag: {} }}", quoted(tag)),
    }
}

/// Generate native host adapters. Guest entry points live in their own module
/// so importing a host binding does not require a canonical guest adapter.
pub fn typescript(world: &World) -> String {
    generate(world, false)
}

/// Generate guest import/export adapters from the same resolved contract.
pub fn typescript_guest(world: &World) -> String {
    generate(world, true)
}

fn generate(world: &World, guest: bool) -> String {
    let generator = Typescript { world };
    let incoming_http = world.exports.iter().find(|interface| {
        interface
            .name
            .starts_with("wasi:http/incoming-handler@0.2.")
            || interface.name == "wasi:http/handler@0.3.0"
    });
    let mut out = String::from("// Generated from WIT by perry-wit. Do not edit.\n");
    if guest {
        out.push_str(
            "import { registerComponentExports, callComponentImport, createComponentResource, getComponentResourceValue } from \"perry/component\";\n",
        );
        out.push_str(
            "import type { OwnedResource, BorrowedResource, ComponentFuture, ComponentStream } from \"perry/component\";\n\n",
        );
    } else {
        out.push_str(
            "import { loadComponent, callComponent, disposeComponent, createComponentHostResource, getComponentHostResourceValue } from \"perry/component\";\n",
        );
        out.push_str("import type { ComponentOptions, ComponentHandle, ComponentAsyncType, OwnedResource, BorrowedResource, ComponentFuture, ComponentStream } from \"perry/component\";\n\n");
        if incoming_http.is_some() {
            out.push_str("import { handleComponentHttpRequest } from \"perry/component\";\nimport type { ComponentHttpServer } from \"perry/component\";\n\n");
        }
    }
    writeln!(out, "export const world = {};", quoted(&world.name)).unwrap();
    writeln!(
        out,
        "const contract = {};\n",
        quoted(&serde_json::to_string(world).expect("world serialization"))
    )
    .unwrap();
    for (key, definition) in &world.types {
        if !generator.is_resource(&definition.ty) {
            writeln!(
                out,
                "export type {} = {};",
                generator.alias(key),
                generator.ty(&definition.ty)
            )
            .unwrap();
        }
    }
    out.push('\n');
    generator.interface(&mut out, "Imports", &world.imports);
    generator.interface(&mut out, "Exports", &world.exports);
    if guest {
        out.push_str("export function register(exports: Exports): void {\n  registerComponentExports(contract, exports);\n}\n\nexport const imports: Imports = ");
        out.push_str(&generator.wrappers(&world.imports, "callComponentImport("));
        out.push_str(";\n\nexport const resources = {\n");
        for interface in &world.exports {
            let resources: Vec<_> = interface
                .resources
                .iter()
                .filter(|(_, key)| key.starts_with('e'))
                .collect();
            if resources.is_empty() {
                continue;
            }
            writeln!(out, "  {}: {{", quoted(&interface.name)).unwrap();
            for (name, key) in resources {
                let id = key.strip_prefix('e').unwrap().parse::<u32>().unwrap() * 2 + 1;
                let tag = quoted(&format!("{}/{key}", world.name));
                writeln!(out, "    {}: {{\n      create: (payload: unknown, destructor?: (payload: unknown) => void): OwnedResource<{tag}> => createComponentResource({id}, payload, destructor),\n      value: (resource: BorrowedResource<{tag}>): unknown => getComponentResourceValue({id}, resource),\n    }},", quoted(name)).unwrap();
            }
            out.push_str("  },\n");
        }
        out.push_str("};\n");
    } else {
        generator.interface(
            &mut out,
            "ApplicationImports",
            &world
                .imports
                .iter()
                .filter(|interface| !interface.name.starts_with("wasi:"))
                .cloned()
                .collect::<Vec<_>>(),
        );
        generator.host_resources(&mut out);
        if incoming_http.is_some() {
            out.push_str("export interface Component { exports: Exports; dispose(): void; handleHttp(server: ComponentHttpServer): Promise<void>; }\n");
        } else {
            out.push_str("export interface Component { exports: Exports; dispose(): void; asyncTypes(): readonly ComponentAsyncType[]; }\n");
        }
        out.push_str("export type ImportBuilder = (resources: HostResources) => Imports | ApplicationImports;\n");
        out.push_str("export function load(bytes: Uint8Array, imports: Imports | ApplicationImports | ImportBuilder, options?: ComponentOptions): Component {\n  const supplied = typeof imports === \"function\" ? (handle: ComponentHandle) => imports(hostResources(handle)) : imports;\n  const handle = loadComponent(bytes, contract, supplied, options);\n  return {\n    dispose: () => disposeComponent(handle),\n    exports: ");
        out.push_str(&generator.wrappers(&world.exports, "callComponent(handle, "));
        if incoming_http.is_none() {
            out.push_str(",\n    asyncTypes: () => handle.asyncTypes(),");
        }
        if let Some(interface) = incoming_http {
            writeln!(out, ",\n    handleHttp: (server: ComponentHttpServer): Promise<void> => handleComponentHttpRequest(server, handle, {}),", quoted(&interface.name)).unwrap();
        }
        out.push_str("\n  };\n}\n\n");
    }
    out
}
