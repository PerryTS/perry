//! Inspect the binary's external contract, rather than scanning embedded text.
//! Runtime validation/execution is a separate acceptance step.
use wasmparser::{ComponentExternalKind, ComponentTypeRef, Encoding, Parser, Payload};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: p3-contract <command.wasm>")?;
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
                    println!("export {}", export.name.name);
                }
            }
            _ => {}
        }
    }
    if depth != 0 || imports == 0 || !command || !random {
        return Err(format!("incomplete P3 entropy command: depth={depth}, imports={imports}, run={command}, random={random}").into());
    }
    println!("PASS: external WASI 0.3-only entropy command contract");
    Ok(())
}
