//! Every input section named `perry_realm` in the ELF objects perry emits is
//! `SHF_LINK_ORDER`: runtime-layout key lists (tied to their descriptors),
//! program-layout key lists (tied to theirs) and static-seed key lists (tied
//! to the seed table). GNU ld before 2.36, the linker of the glibc 2.31
//! release floor (binutils 2.35), fails a link whose output section has both
//! ordered and unordered inputs: "perry_realm has both ordered [...] and
//! unordered [...] sections", "final link failed: bad value". Checked on the
//! objects, so the test needs no old linker.
#![cfg(target_os = "linux")]

use object::read::elf::{ElfFile64, FileHeader, SectionHeader};
use object::{Endianness, Object, ObjectSection, ObjectSymbol};
use std::process::Command;

// A class (program layout), an object literal (static seed) and a RegExp
// exec result with named groups (runtime layouts).
const SOURCE: &str = r#"
class Point { x = 1; y = 2; sum(): number { return this.x + this.y; } }
const p = new Point();
const o = { alpha: 1, beta: "two", gamma: [3] };
const m = /(?<word>[a-z]+)-(?<num>\d+)/.exec("abc-42");
console.log(p.sum(), o.alpha, o.beta, m?.groups?.word, m?.index);
"#;

#[test]
fn every_realm_section_input_is_link_ordered() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("realm.ts");
    std::fs::write(&input, SOURCE).unwrap();
    let objects = dir.path().join("objs");
    std::fs::create_dir_all(&objects).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&input)
        .arg("--no-link")
        .arg("-o")
        .arg(objects.join("realm.o"))
        .arg("--no-cache")
        .output()
        .expect("run perry");
    assert!(
        result.status.success(),
        "compile:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let mut ordered = Vec::new();
    let mut untied = Vec::new();
    for entry in std::fs::read_dir(&objects).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "o") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let file = ElfFile64::<Endianness>::parse(&*data).unwrap();
        let endian = file.elf_header().endian().unwrap();
        for section in file.sections() {
            if section.name() != Ok("perry_realm") {
                continue;
            }
            // The cells this input section holds, to say which kinds were seen.
            let cells: Vec<String> = file
                .symbols()
                .filter(|symbol| symbol.section_index() == Some(section.index()))
                .filter_map(|symbol| symbol.name().ok().map(str::to_string))
                .filter(|name| !name.is_empty())
                .collect();
            let header = section.elf_section_header();
            let flags = header.sh_flags(endian);
            if flags & u64::from(object::elf::SHF_LINK_ORDER) != 0 && header.sh_link(endian) != 0 {
                ordered.extend(cells);
            } else {
                untied.push(format!(
                    "{} section #{}: {cells:?}",
                    path.display(),
                    section.index().0
                ));
            }
        }
    }
    assert!(
        untied.is_empty(),
        "perry_realm inputs without SHF_LINK_ORDER (GNU ld < 2.36 rejects the mix): {untied:#?}"
    );
    // Each kind of cell was emitted, so the check above covered it.
    for kind in [
        "perry_runtime_layout_",
        "_image_keys",
        "perry_static_seed_list_",
    ] {
        assert!(
            ordered.iter().any(|name| name.contains(kind)),
            "no tied {kind} cell in the emitted objects: {ordered:#?}"
        );
    }
}
