//! Append callback interposition without rewriting original indices or code.
//! Imported/nested function implementations are left untracked for now.
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::{
    reencode::{ReencodeComponent, RoundtripReencoder},
    Alias, CanonicalFunctionSection, Component, ComponentAliasSection, ComponentExportSection,
    ComponentImportSection, ComponentInstanceSection, ComponentSectionId, ComponentTypeSection,
    ExportKind, InstanceSection, ModuleArg, RawSection,
};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExternalKind as Kind, ComponentInstance,
    ComponentTypeRef, Parser, Payload, ValType, Validator, WasmFeatures,
};

pub(super) struct Instrumented {
    pub bytes: Vec<u8>,
    pub hook: String,
}

pub(super) fn instrument(
    bytes: &[u8],
    interface: &str,
    name: &str,
) -> Result<Option<Instrumented>> {
    let types = Validator::new_with_features(WasmFeatures::all()).validate_all(bytes)?;
    let types = types.as_ref();
    let mut depth = 0;
    let mut functions = 0;
    let mut instances = 0;
    let mut lifts = BTreeMap::new();
    let mut fields = BTreeMap::new();
    let mut selected = None;
    let mut names = BTreeSet::new();
    let mut sections = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload?;
        if depth > 0 {
            match payload {
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) => depth -= 1,
                _ => {}
            }
            continue;
        }
        match &payload {
            Payload::ComponentImportSection(section) => {
                for import in section.clone() {
                    let import = import?;
                    names.insert(import.name.name.to_owned());
                    match import.ty {
                        ComponentTypeRef::Func(_) => functions += 1,
                        ComponentTypeRef::Instance(_) => instances += 1,
                        _ => {}
                    }
                }
            }
            Payload::ComponentAliasSection(section) => {
                for alias in section.clone() {
                    if let ComponentAlias::InstanceExport {
                        kind,
                        instance_index,
                        name,
                    } = alias?
                    {
                        match kind {
                            Kind::Func => {
                                if let Some(index) =
                                    field(&fields, instance_index, name, Kind::Func)
                                {
                                    if let Some(lift) = lifts.get(&index).cloned() {
                                        lifts.insert(functions, lift);
                                    }
                                }
                                functions += 1;
                            }
                            Kind::Instance => instances += 1,
                            _ => {}
                        }
                    }
                }
            }
            Payload::ComponentInstanceSection(section) => {
                for instance in section.clone() {
                    if let ComponentInstance::FromExports(exports) = instance? {
                        fields.insert(instances, exports);
                    }
                    instances += 1;
                }
            }
            Payload::ComponentCanonicalSection(section) => {
                for canonical in section.clone() {
                    if let lift @ CanonicalFunction::Lift { .. } = canonical? {
                        lifts.insert(functions, lift);
                        functions += 1;
                    }
                }
            }
            Payload::ComponentExportSection(section) => {
                for export in section.clone() {
                    let export = export?;
                    names.insert(export.name.name.to_owned());
                    if (interface.is_empty()
                        && export.kind == Kind::Func
                        && export.name.name == name)
                        || (!interface.is_empty()
                            && export.kind == Kind::Instance
                            && export.name.name == interface)
                    {
                        selected = Some(export.clone());
                    }
                    // Exports introduce aliases, too. Keep their index spaces.
                    match export.kind {
                        Kind::Func => {
                            if let Some(lift) = lifts.get(&export.index).cloned() {
                                lifts.insert(functions, lift);
                            }
                            functions += 1;
                        }
                        Kind::Instance => {
                            if let Some(exports) = fields.get(&export.index).cloned() {
                                fields.insert(instances, exports);
                            }
                            instances += 1;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if matches!(
            payload,
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. }
        ) {
            depth += 1;
        }
        sections.push(payload);
    }
    ensure!(
        functions == types.component_function_count()
            && instances == types.component_instance_count(),
        "unhandled component index definition"
    );
    let Some(selected) = selected else {
        return Ok(None);
    };
    let selected_function = if interface.is_empty() {
        Some(selected.index)
    } else {
        field(&fields, selected.index, name, Kind::Func)
    };
    let Some(CanonicalFunction::Lift {
        core_func_index,
        type_index,
        options,
    }) = selected_function.and_then(|index| lifts.remove(&index))
    else {
        return Ok(None);
    };
    let Some(callback) = options.iter().find_map(|option| match option {
        wasmparser::CanonicalOption::Callback(index) => Some(*index),
        _ => None,
    }) else {
        return Ok(None);
    };
    let entry_type = types[types.core_function_at(core_func_index)].unwrap_func();
    if entry_type.results() != [ValType::I32]
        || entry_type.params().iter().any(|ty| {
            !matches!(
                ty,
                ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
            )
        })
    {
        return Ok(None);
    }
    let mut fresh = |prefix: &str| {
        (0..)
            .map(|i| format!("perry-{prefix}-{i}"))
            .find(|name| names.insert(name.clone()))
            .unwrap()
    };
    let hook = fresh("task-exit");
    let private_export = fresh("untracked-export");
    let mut output = Component::new();
    let mut reencoder = RoundtripReencoder;
    for payload in sections {
        if let Payload::ComponentExportSection(section) = payload {
            let mut exports = ComponentExportSection::new();
            for export in section {
                let export = export?;
                if export.name.name == selected.name.name && export.kind == selected.kind {
                    exports.export(
                        private_export.as_str(),
                        export.kind.into(),
                        export.index,
                        export
                            .ty
                            .map(|ty| reencoder.component_type_ref(ty))
                            .transpose()?,
                    );
                } else {
                    reencoder.parse_component_export(&mut exports, export)?;
                }
            }
            output.section(&exports);
        } else if let Some((id, range)) = payload.as_section() {
            output.section(&RawSection {
                id,
                data: &bytes[range.start as usize..range.end as usize],
            });
        }
    }
    let mut params = String::new();
    let mut args = String::new();
    for (index, param) in entry_type.params().iter().enumerate() {
        let ty = match param {
            ValType::I32 => "i32",
            ValType::I64 => "i64",
            ValType::F32 => "f32",
            ValType::F64 => "f64",
            _ => unreachable!(),
        };
        params.push_str(&format!(" (param {ty})"));
        args.push_str(&format!(" (local.get {index})"));
    }
    let wrapper = wat::parse_str(format!(
        r#"(module
        (import "original" "run" (func $run {params} (result i32)))
        (import "original" "callback" (func $callback (param i32 i32 i32) (result i32)))
        (import "original" "exited" (func $exited))
        (func (export "run") {params} (result i32) (local $status i32)
          (local.set $status (call $run {args}))
          (if (i32.eqz (i32.and (local.get $status) (i32.const 15))) (then (call $exited)))
          (local.get $status))
        (func (export "callback") (param i32 i32 i32) (result i32) (local $status i32)
          (local.set $status (call $callback (local.get 0) (local.get 1) (local.get 2)))
          (if (i32.eqz (i32.and (local.get $status) (i32.const 15))) (then (call $exited)))
          (local.get $status)))"#
    ))?;
    let function = types.component_function_count();
    let core = types.function_count();
    let instance = types.core_instance_count();
    let mut hook_type = ComponentTypeSection::new();
    hook_type
        .function()
        .params(std::iter::empty::<(&str, wasm_encoder::ComponentValType)>())
        .result(None);
    output.section(&hook_type);
    output.section(ComponentImportSection::new().import(
        hook.as_str(),
        wasm_encoder::ComponentTypeRef::Func(types.component_type_count()),
    ));
    output.section(CanonicalFunctionSection::new().lower(function, []));
    output.section(&RawSection {
        id: ComponentSectionId::CoreModule.into(),
        data: &wrapper,
    });
    let mut core_instances = InstanceSection::new();
    core_instances.export_items([
        ("run", ExportKind::Func, core_func_index),
        ("callback", ExportKind::Func, callback),
        ("exited", ExportKind::Func, core),
    ]);
    core_instances.instantiate(
        types.module_count(),
        [("original", ModuleArg::Instance(instance))],
    );
    output.section(&core_instances);
    let mut aliases = ComponentAliasSection::new();
    for name in ["run", "callback"] {
        aliases.alias(Alias::CoreInstanceExport {
            instance: instance + 1,
            kind: ExportKind::Func,
            name,
        });
    }
    output.section(&aliases);
    let options = options
        .iter()
        .map(|option| match option {
            wasmparser::CanonicalOption::Callback(_) => {
                Ok(wasm_encoder::CanonicalOption::Callback(core + 2))
            }
            option => reencoder.canonical_option(*option),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    output.section(CanonicalFunctionSection::new().lift(core + 1, type_index, options));
    let mut selected = selected;
    selected.index = if interface.is_empty() {
        function + 1
    } else {
        let exports = fields.get(&selected.index).unwrap();
        let mut instance = ComponentInstanceSection::new();
        instance.export_items(exports.iter().map(|export| {
            (
                export.name,
                export.kind.into(),
                if export.kind == Kind::Func && export.name.name == name {
                    function + 1
                } else {
                    export.index
                },
            )
        }));
        output.section(&instance);
        types.component_instance_count()
    };
    let mut exports = ComponentExportSection::new();
    reencoder.parse_component_export(&mut exports, selected)?;
    output.section(&exports);
    let bytes = output.finish();
    Validator::new_with_features(WasmFeatures::all()).validate_all(&bytes)?;
    Ok(Some(Instrumented { bytes, hook }))
}

fn field(
    fields: &BTreeMap<u32, Box<[wasmparser::ComponentExport<'_>]>>,
    instance: u32,
    name: &str,
    kind: Kind,
) -> Option<u32> {
    fields
        .get(&instance)?
        .iter()
        .find(|export| export.name.name == name && export.kind == kind)
        .map(|export| export.index)
}
