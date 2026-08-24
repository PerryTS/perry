//! Source-contract extraction for the public `perry/native` POD profile.

use std::collections::HashSet;

use perry_api_manifest::{NativeAbiType, NativePodAbi, NativePodFieldAbi};

use crate::{types::Type, Interface, Module};

/// Resolve an exported `pod<T>` alias into the canonical native-library POD
/// descriptor used by manifest validation and code generation.
pub fn exported_native_pod_abi(module: &Module, export_name: &str) -> Result<NativePodAbi, String> {
    let alias = module
        .type_aliases
        .iter()
        .find(|alias| alias.is_exported && alias.name == export_name)
        .ok_or_else(|| format!("source export `{export_name}` is not an exported type alias"))?;
    if !alias.type_params.is_empty() {
        return Err(format!(
            "source export `{export_name}` must not declare type parameters"
        ));
    }
    let Type::Generic { base, type_args } = &alias.ty else {
        return Err(format!(
            "source export `{export_name}` must be declared as pod<T>"
        ));
    };
    if base != "PerryPod" || type_args.len() != 1 {
        return Err(format!(
            "source export `{export_name}` must be declared as pod<T>"
        ));
    }

    let mut resolving = HashSet::new();
    let fields = pod_fields(module, &type_args[0], &mut resolving)?;
    if fields.is_empty() {
        return Err(format!(
            "source POD `{export_name}` must contain at least one field"
        ));
    }
    Ok(NativePodAbi {
        name: Some(export_name.to_string()),
        fields,
    })
}

fn pod_fields(
    module: &Module,
    ty: &Type,
    resolving: &mut HashSet<String>,
) -> Result<Vec<NativePodFieldAbi>, String> {
    match ty {
        Type::Object(object) => {
            if object.index_signature.is_some() {
                return Err("POD object types cannot contain an index signature".to_string());
            }
            let order = object.property_order.as_ref().ok_or_else(|| {
                "POD object type is missing stable source property order".to_string()
            })?;
            order
                .iter()
                .map(|name| {
                    let property = object.properties.get(name).ok_or_else(|| {
                        format!("POD property `{name}` is missing from its object type")
                    })?;
                    if property.optional {
                        return Err(format!("POD field `{name}` must not be optional"));
                    }
                    Ok(NativePodFieldAbi {
                        name: name.clone(),
                        ty: pod_field_type(module, &property.ty, resolving)?,
                    })
                })
                .collect()
        }
        Type::Named(name) => {
            if !resolving.insert(name.clone()) {
                return Err(format!(
                    "recursive POD source type `{name}` is not supported"
                ));
            }
            let result = if let Some(alias) = module.type_aliases.iter().find(|a| a.name == *name) {
                if !alias.type_params.is_empty() {
                    Err(format!(
                        "POD source type alias `{name}` must not be generic"
                    ))
                } else {
                    pod_fields(module, &alias.ty, resolving)
                }
            } else if let Some(interface) = module.interfaces.iter().find(|i| i.name == *name) {
                interface_fields(module, interface, resolving)
            } else {
                Err(format!("POD source type `{name}` could not be resolved"))
            };
            resolving.remove(name);
            result
        }
        _ => Err("pod<T> requires an object literal, interface, or object type alias".to_string()),
    }
}

fn interface_fields(
    module: &Module,
    interface: &Interface,
    resolving: &mut HashSet<String>,
) -> Result<Vec<NativePodFieldAbi>, String> {
    if !interface.type_params.is_empty() || !interface.extends.is_empty() {
        return Err(format!(
            "POD interface `{}` must not be generic or extend another interface",
            interface.name
        ));
    }
    if !interface.methods.is_empty() {
        return Err(format!(
            "POD interface `{}` must not contain methods",
            interface.name
        ));
    }
    interface
        .properties
        .iter()
        .map(|property| {
            if property.optional {
                return Err(format!(
                    "POD field `{}` must not be optional",
                    property.name
                ));
            }
            Ok(NativePodFieldAbi {
                name: property.name.clone(),
                ty: pod_field_type(module, &property.ty, resolving)?,
            })
        })
        .collect()
}

fn pod_field_type(
    module: &Module,
    ty: &Type,
    resolving: &mut HashSet<String>,
) -> Result<NativeAbiType, String> {
    let scalar = match ty {
        Type::Number => Some(NativeAbiType::F64),
        Type::Named(name) => match name.as_str() {
            "PerryI8" => Some(NativeAbiType::I8),
            "PerryI16" => Some(NativeAbiType::I16),
            "PerryI32" => Some(NativeAbiType::I32),
            "PerryI64" => Some(NativeAbiType::I64),
            "PerryU8" | "PerryByte" => Some(NativeAbiType::U8),
            "PerryU16" => Some(NativeAbiType::U16),
            "PerryU32" => Some(NativeAbiType::U32),
            "PerryU64" => Some(NativeAbiType::U64),
            "PerryISize" => Some(NativeAbiType::ISize),
            "PerryUSize" => Some(NativeAbiType::USize),
            "PerryF32" => Some(NativeAbiType::F32),
            "PerryF64" => Some(NativeAbiType::F64),
            "PerryBufferLen" => Some(NativeAbiType::BufferLen),
            "PerryHandleId" => Some(NativeAbiType::HandleId),
            _ => None,
        },
        _ => None,
    };
    if let Some(scalar) = scalar {
        return Ok(scalar);
    }

    match ty {
        Type::Generic { base, type_args } if base == "PerryPod" && type_args.len() == 1 => {
            Ok(NativeAbiType::Pod(NativePodAbi {
                name: None,
                fields: pod_fields(module, &type_args[0], resolving)?,
            }))
        }
        Type::Named(name) => {
            if !resolving.insert(name.clone()) {
                return Err(format!(
                    "recursive POD source type `{name}` is not supported"
                ));
            }
            let result = if let Some(alias) = module.type_aliases.iter().find(|a| a.name == *name) {
                if !alias.type_params.is_empty() {
                    Err(format!("POD field type alias `{name}` must not be generic"))
                } else {
                    pod_field_type(module, &alias.ty, resolving)
                }
            } else {
                Err(format!(
                    "POD field type `{name}` is not a fixed-width scalar or nested pod<T>"
                ))
            };
            resolving.remove(name);
            result
        }
        _ => Err(format!(
            "POD field type `{ty:?}` is not a fixed-width scalar or nested pod<T>"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> Module {
        let ast = perry_parser::parse_typescript(source, "native-contract.ts").expect("parse");
        crate::set_current_module_source(source.to_string());
        let module = crate::lower_module(&ast, "native-contract", "native-contract.ts");
        crate::clear_current_module_source();
        module.expect("lower")
    }

    #[test]
    fn extracts_all_public_scalar_widths_and_nested_pods_in_source_order() {
        let module = lower(
            r#"
import type { pod, i8, i16, i32, i64, u8, byte, u16, u32, u64, isize, usize, f32, f64 } from "perry/native";
interface Nested { code: u16; weight: f32 }
export type Packet = pod<{
  a: i8; b: i16; c: i32; d: i64;
  e: u8; f: byte; g: u16; h: u32; i: u64;
  j: isize; k: usize; l: f32; m: f64; n: number;
  nested: pod<Nested>;
}>;
"#,
        );

        let pod = exported_native_pod_abi(&module, "Packet").expect("extract POD");
        let names: Vec<_> = pod.fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(
            names,
            ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "nested"]
        );
        assert_eq!(pod.fields[0].ty, NativeAbiType::I8);
        assert_eq!(pod.fields[5].ty, NativeAbiType::U8);
        assert_eq!(pod.fields[9].ty, NativeAbiType::ISize);
        assert_eq!(pod.fields[13].ty, NativeAbiType::F64);
        let NativeAbiType::Pod(nested) = &pod.fields[14].ty else {
            panic!("nested field must be a POD")
        };
        assert_eq!(nested.fields[0].ty, NativeAbiType::U16);
        assert_eq!(nested.fields[1].ty, NativeAbiType::F32);
    }

    #[test]
    fn requires_an_exported_closed_pod_alias() {
        let module = lower(
            r#"
import type { pod, u32 } from "perry/native";
type Hidden = pod<{ value: u32 }>;
export type Optional = pod<{ value?: u32 }>;
"#,
        );
        assert!(exported_native_pod_abi(&module, "Hidden")
            .unwrap_err()
            .contains("not an exported type alias"));
        assert!(exported_native_pod_abi(&module, "Optional")
            .unwrap_err()
            .contains("must not be optional"));
    }
}
