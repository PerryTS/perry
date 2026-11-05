//! #13649: compare actual renderer machine code for equivalent loop updates.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = include_str!("../../../test-files/test_parity_unroll_renderer_size.ts");

fn compile(dir: &Path, name: &str, source: &str) -> PathBuf {
    let input = dir.join(format!("{name}.ts"));
    let binary = dir.join(name);
    std::fs::write(&input, source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .arg("compile")
        .arg(&input)
        .arg("-o")
        .arg(&binary)
        .args([
            "--no-cache",
            "--no-auto-optimize",
            "--strict-eval",
            "--strict-dynamic-import",
            "--strict-unimplemented",
        ])
        .env("PERRY_KEEP_SYMBOLS", "1")
        .env_remove("PERRY_LL_OPT_LEVEL")
        .output()
        .expect("compile renderer");
    assert!(
        result.status.success(),
        "compile {name}:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    binary
}

fn largest_renderer(binary: &Path) -> u64 {
    let data = std::fs::read(binary).unwrap();
    let file = object::File::parse(&*data).unwrap();
    let symbols: Vec<_> = file
        .symbols()
        .filter(|s| s.kind() == SymbolKind::Text && s.section_index().is_some())
        .collect();
    let sizes: Vec<_> = symbols
        .iter()
        .filter(|s| s.name().is_ok_and(|name| name.contains("Table__render")))
        .map(|s| {
            if s.size() != 0 {
                return s.size();
            }
            // Mach-O has no symbol sizes. Infer the extent from the next
            // distinct code address in this section, ignoring aliases.
            let section = file.section_by_index(s.section_index().unwrap()).unwrap();
            let end = symbols
                .iter()
                .filter(|next| {
                    next.section_index() == s.section_index() && next.address() > s.address()
                })
                .map(|next| next.address())
                .min()
                .unwrap_or(section.address() + section.size());
            end - s.address()
        })
        .collect();
    assert!(
        !sizes.is_empty(),
        "renderer symbols must actually be present"
    );
    assert!(sizes.iter().all(|&size| size > 0));
    *sizes.iter().max().unwrap()
}

fn expected_table(seed: i32, color: bool) -> String {
    let mut output = String::new();
    for row in 0..8 {
        output.push_str(&format!("{row}:"));
        for col in 0..8 {
            let index = row * 8 + col;
            let value = (seed + index * 17) % 100;
            let selected = index == seed & 63;
            if color {
                let background = if selected {
                    100
                } else if value > 70 {
                    52
                } else if (row + col) & 1 == 0 {
                    24
                } else {
                    30
                };
                output.push_str(&format!("\x1b[48;5;{background}m {value:02} \x1b[0m"));
            } else if selected {
                output.push_str(&format!("[{value:02}]"));
            } else {
                output.push_str(&format!(" {value:02} "));
            }
        }
        output.push('\n');
    }
    output
}

#[test]
fn nested_renderer_updates_have_comparable_native_code_size() {
    let dir = tempfile::tempdir().unwrap();
    let increment = compile(dir.path(), "increment", SOURCE);
    let indexed_source = SOURCE
        .replace("row++", "row += 1")
        .replace("col++", "col += 1");
    assert_ne!(SOURCE, indexed_source);
    let indexed = compile(dir.path(), "indexed", &indexed_source);

    for seed in [0, 7, 63, 123] {
        for color in [false, true] {
            let expected = expected_table(seed, color);
            for binary in [&increment, &indexed] {
                let result = Command::new(binary)
                    .arg(seed.to_string())
                    .arg(if color { "color" } else { "plain" })
                    .output()
                    .expect("run renderer");
                assert!(result.status.success(), "{result:?}");
                assert_eq!(
                    result.stdout,
                    expected.as_bytes(),
                    "{binary:?}, {seed}, {color}"
                );
            }
        }
    }

    let increment_bytes = largest_renderer(&increment);
    let indexed_bytes = largest_renderer(&indexed);
    eprintln!("largest Table.render: ++ {increment_bytes} B; += 1 {indexed_bytes} B");
    // Allow normal backend/platform variation, but catch the reported 33x
    // blow-up. Inspect renderer symbols, not the runtime-dominated executable.
    assert!(
        increment_bytes <= indexed_bytes * 2 + 1024,
        "++ renderer grew to {increment_bytes} B versus += 1 at {indexed_bytes} B"
    );
}
