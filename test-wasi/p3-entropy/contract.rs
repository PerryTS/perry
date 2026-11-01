//! Inspect the binary's external contract, rather than scanning embedded text.
//! Runtime validation/execution is a separate acceptance step.
use wasmparser::{ComponentExternalKind, ComponentTypeRef, Encoding, Parser, Payload, TypeBounds};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let reactor = args.first().is_some_and(|arg| arg == "--reactor");
    let path = args.get(usize::from(reactor)).ok_or(
        "usage: p3-contract <command.wasm> | --reactor <reactor.wasm> <required-export>...",
    )?;
    let mut required = if reactor {
        args[2..].to_vec()
    } else {
        Vec::new()
    };
    let mut types = Vec::new();
    required.retain(|arg| {
        if let Some(name) = arg.strip_prefix("--type-import=") {
            types.push(name.to_string());
            false
        } else {
            true
        }
    });
    if types
        .iter()
        .any(|name| name.is_empty() || name.starts_with("wasi:"))
    {
        return Err("invalid declared application type import".into());
    }
    if (reactor && required.is_empty()) || (!reactor && args.len() != 1) {
        return Err("invalid P3 contract profile arguments".into());
    }
    let bytes = std::fs::read(path)?;
    let mut depth = 0usize;
    let mut imports = 0;
    let mut command = false;
    let mut random = false;
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload? {
            Payload::Version { encoding, .. } => {
                if depth == 0 && encoding != Encoding::Component {
                    return Err("expected a component, not a core module".into());
                }
                depth += 1;
            }
            Payload::End(_) => depth = depth.checked_sub(1).ok_or("unbalanced component")?,
            Payload::ComponentImportSection(section) if depth == 1 => {
                for import in section {
                    let import = import?;
                    let name = import.name.name;
                    // A typed reactor can also import the selected world's
                    // concrete structural types. Require exact declarations
                    // and equality bounds; no unlisted type or resource import
                    // can bypass the WASI version/interface checks below.
                    if reactor && matches!(import.ty, ComponentTypeRef::Type(TypeBounds::Eq(_))) {
                        if let Some(index) = types.iter().position(|ty| ty == name) {
                            types.remove(index);
                            println!("declared application type {name}");
                            continue;
                        }
                    }
                    if !matches!(import.ty, ComponentTypeRef::Instance(_))
                        || !name.starts_with("wasi:")
                        || !name.ends_with("@0.3.0")
                    {
                        return Err(
                            format!("non-P3 external import: {name} ({:?})", import.ty).into()
                        );
                    }
                    random |= name == "wasi:random/random@0.3.0";
                    imports += 1;
                    println!("import {name}");
                }
            }
            Payload::ComponentExportSection(section) if depth == 1 => {
                for export in section {
                    let export = export?;
                    command |= export.name.name == "wasi:cli/run@0.3.0"
                        && export.kind == ComponentExternalKind::Instance;
                    if export.kind == ComponentExternalKind::Func {
                        required.retain(|name| name != export.name.name);
                    }
                    println!("export {}", export.name.name);
                }
            }
            _ => {}
        }
    }
    if depth != 0
        || imports == 0
        || (!reactor && !command)
        || (reactor && command)
        || !random
        || !required.is_empty()
        || !types.is_empty()
    {
        return Err(format!("incomplete P3 contract: depth={depth}, imports={imports}, run={command}, random={random}, missing_exports={required:?}, missing_types={types:?}").into());
    }
    if reactor {
        println!("PASS: external WASI 0.3-only reactor contract and required function exports");
    } else {
        println!("PASS: external WASI 0.3-only entropy command contract");
    }
    Ok(())
}
