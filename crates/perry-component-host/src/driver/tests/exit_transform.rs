//! Test-only binary interposition proof. No product loader uses this yet.
use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
use wasm_encoder::{
    reencode::{ReencodeComponent, RoundtripReencoder},
    Alias, CanonicalFunctionSection, Component, ComponentAliasSection, ComponentExportSection,
    ComponentSectionId, ExportKind, InstanceSection, ModuleArg, RawSection,
};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExternalKind, ComponentTypeRef, Parser, Payload,
    ValType, Validator, WasmFeatures,
};

pub(super) fn wrap_root_function(
    bytes: &[u8],
    name: &str,
    hook_instance: u32,
    hook_name: &str,
) -> Result<Vec<u8>> {
    let types = Validator::new_with_features(WasmFeatures::all()).validate_all(bytes)?;
    let types = types.as_ref();
    let mut depth = 0;
    let mut functions = 0;
    let mut lifts = BTreeMap::new();
    let mut selected = None;
    let mut output = Component::new();
    let mut reencoder = RoundtripReencoder;
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
                    if matches!(import?.ty, ComponentTypeRef::Func(_)) {
                        functions += 1;
                    }
                }
            }
            Payload::ComponentAliasSection(section) => {
                for alias in section.clone() {
                    if matches!(
                        alias?,
                        ComponentAlias::InstanceExport {
                            kind: ComponentExternalKind::Func,
                            ..
                        }
                    ) {
                        functions += 1;
                    }
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
                let mut remaining = ComponentExportSection::new();
                for export in section.clone() {
                    let export = export?;
                    // Component exports introduce aliases into their index
                    // spaces. Preserve that definition while changing its
                    // public name; later original indices must stay stable.
                    if export.kind == ComponentExternalKind::Func {
                        functions += 1;
                    }
                    if export.name.name == name && export.kind == ComponentExternalKind::Func {
                        ensure!(selected.is_none(), "duplicate selected export");
                        selected = Some(export.clone());
                        remaining.export(
                            "perry-untracked-export-0",
                            export.kind.into(),
                            export.index,
                            export
                                .ty
                                .map(|ty| reencoder.component_type_ref(ty))
                                .transpose()?,
                        );
                    } else {
                        reencoder.parse_component_export(&mut remaining, export)?;
                    }
                }
                if !remaining.is_empty() {
                    output.section(&remaining);
                }
                continue;
            }
            _ => {}
        }
        if let Some((id, range)) = payload.as_section() {
            output.section(&RawSection {
                id,
                data: &bytes[range.start as usize..range.end as usize],
            });
        }
        if matches!(
            payload,
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. }
        ) {
            depth += 1;
        }
    }
    ensure!(
        functions == types.component_function_count(),
        "unhandled function definition"
    );
    let selected = selected.context("root function export missing")?;
    let CanonicalFunction::Lift {
        core_func_index,
        type_index,
        options,
    } = lifts
        .remove(&selected.index)
        .context("export must be a local canonical lift")?
    else {
        unreachable!()
    };
    let callback = options
        .iter()
        .find_map(|option| match option {
            wasmparser::CanonicalOption::Callback(index) => Some(*index),
            _ => None,
        })
        .context("prototype requires callback ABI")?;
    let entry_type = types[types.core_function_at(core_func_index)].unwrap_func();
    ensure!(
        entry_type.results() == [ValType::I32],
        "callback entry must return i32"
    );
    let mut params = String::new();
    let mut args = String::new();
    for (index, param) in entry_type.params().iter().enumerate() {
        let ty = match param {
            ValType::I32 => "i32",
            ValType::I64 => "i64",
            ValType::F32 => "f32",
            ValType::F64 => "f64",
            _ => bail!("unsupported entry parameter"),
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
    output.section(ComponentAliasSection::new().alias(Alias::InstanceExport {
        instance: hook_instance,
        kind: wasm_encoder::ComponentExportKind::Func,
        name: hook_name,
    }));
    output.section(CanonicalFunctionSection::new().lower(function, []));
    output.section(&RawSection {
        id: ComponentSectionId::CoreModule.into(),
        data: &wrapper,
    });
    let mut instances = InstanceSection::new();
    instances.export_items([
        ("run", ExportKind::Func, core_func_index),
        ("callback", ExportKind::Func, callback),
        ("exited", ExportKind::Func, core),
    ]);
    instances.instantiate(
        types.module_count(),
        [("original", ModuleArg::Instance(instance))],
    );
    output.section(&instances);
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
    selected.index = function + 1;
    let mut exports = ComponentExportSection::new();
    reencoder.parse_component_export(&mut exports, selected)?;
    output.section(&exports);
    let bytes = output.finish();
    Validator::new_with_features(WasmFeatures::all()).validate_all(&bytes)?;
    Ok(bytes)
}
