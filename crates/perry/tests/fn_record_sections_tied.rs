//! Every input section named `perry_fnrec` in an emitted ELF object is
//! `SHF_LINK_ORDER` (DCE layer 0's per-body records and the image sentinel).
//! GNU ld before 2.36, the linker of the glibc 2.31 release floor (binutils
//! 2.35), fails a link whose output section has both ordered and unordered
//! inputs: "perry_fnrec has both ordered [...] and unordered [...] sections",
//! "final link failed: bad value". Checked on the objects, so the test needs
//! no old linker.
#![cfg(target_os = "linux")]

use object::read::elf::{ElfFile64, FileHeader, SectionHeader};
use object::{Endianness, Object, ObjectSection};
use std::process::Command;

const SOURCE: &str = r#"
class Counter {
  n = 0;
  inc(): number { return ++this.n; }
}
function unused(x: number): number { return x * 31 + 7; }
const c = new Counter();
const xs = [1, 2, 3].map((v) => v * 2 + c.inc());
console.log(xs.join(","));
"#;

#[test]
fn every_records_section_input_is_link_ordered() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("fnrec.ts");
    std::fs::write(&input, SOURCE).unwrap();
    let objects = dir.path().join("objs");
    std::fs::create_dir_all(&objects).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&input)
        .arg("--no-link")
        .arg("-o")
        .arg(objects.join("fnrec.o"))
        .arg("--no-cache")
        .output()
        .expect("run perry");
    assert!(
        result.status.success(),
        "compile:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let mut ordered = 0usize;
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
            if section.name() != Ok("perry_fnrec") {
                continue;
            }
            let header = section.elf_section_header();
            let flags = header.sh_flags(endian);
            if flags & u64::from(object::elf::SHF_LINK_ORDER) != 0 && header.sh_link(endian) != 0 {
                ordered += 1;
            } else {
                untied.push(format!("{} section #{}", path.display(), section.index().0));
            }
        }
    }
    // At least one body record and the sentinel.
    assert!(ordered >= 2, "no tied perry_fnrec sections found (ordered = {ordered})");
    assert!(
        untied.is_empty(),
        "perry_fnrec inputs without SHF_LINK_ORDER (GNU ld < 2.36 rejects the mix): {untied:#?}"
    );
}
