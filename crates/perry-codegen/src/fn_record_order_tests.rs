//! Link a real record section, then reject the same fixture with its ordering
//! association removed. An IR-text assertion alone cannot test linker order.
use super::*;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn linked_records(dir: &Path, associated: bool) -> Vec<u64> {
    let object = dir.join(if associated { "ordered.o" } else { "broken.o" });
    let binary = object.with_extension("elf");
    let dump = object.with_extension("records");
    let mut ir = String::from("target triple = \"x86_64-unknown-linux-gnu\"\n");
    for body in ["first", "second"] {
        ir.push_str(&format!("define void @{body}() section \".text.{body}\" {{ ret void }}\n@{body}$info = constant ptr @{body}\n"));
    }
    for (_, line) in image_lines(RecordFormat::Elf) {
        let line = if associated {
            line
        } else {
            line.split(", !associated").next().unwrap().to_string()
        };
        ir.push_str(&line);
        ir.push('\n');
    }
    let records: Vec<_> = ["second", "first"]
        .into_iter()
        .map(|body| {
            let line = record_line(RecordFormat::Elf, body);
            if associated {
                line
            } else {
                line.split(", !associated").next().unwrap().to_string()
            }
        })
        .collect();
    for line in &records {
        ir.push_str(line);
        ir.push('\n');
    }
    push_compiler_used(&mut ir, records.iter().map(String::as_str));
    let mut clang = Command::new("clang")
        .args(["-x", "ir", "-c", "-o"])
        .arg(&object)
        .arg("-")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("clang");
    clang
        .stdin
        .take()
        .unwrap()
        .write_all(ir.as_bytes())
        .unwrap();
    let result = clang.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new("clang")
        .args([
            "-nostdlib",
            "-no-pie",
            "-Wl,-e,first,-u,second,-u,perry_fnrec_bounds,--gc-sections",
        ])
        .arg(&object)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new("objcopy")
        .arg("--dump-section")
        .arg(format!("perry_fnrec={}", dump.display()))
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new("nm")
        .args(["-n", "--defined-only"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(result.status.success());
    let symbols: Vec<_> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let words: Vec<_> = line.split_whitespace().collect();
            (words.len() == 3).then(|| {
                (
                    words[2].to_string(),
                    u64::from_str_radix(words[0], 16).unwrap(),
                )
            })
        })
        .collect();
    let symbol = |name: &str| symbols.iter().find(|s| s.0 == name).unwrap().1;
    let start = symbol("__start_perry_fnrec");
    let bytes = fs::read(&dump).unwrap();
    assert_eq!(bytes.len(), 12, "two body records and one sentinel");
    let sentinel = symbol("perry_fnrec_sentinel");
    let mut codes = Vec::new();
    for (index, word) in bytes.chunks_exact(4).enumerate() {
        let address = start + index as u64 * 4;
        let delta = i32::from_le_bytes(word.try_into().unwrap());
        if address == sentinel {
            assert_eq!(delta, 0);
            continue;
        }
        assert_ne!(delta, 0);
        let info = address.wrapping_add_signed(delta as i64);
        let body = ["first", "second"]
            .into_iter()
            .find(|body| symbol(&format!("{body}$info")) == info)
            .expect("record info");
        codes.push(symbol(body));
    }
    assert_eq!(codes.len(), 2, "the sentinel is present and excluded");
    codes
}

#[test]
fn emitted_elf_records_follow_code_order_and_include_sentinel() {
    let dir = std::env::temp_dir().join(format!("perry-fnrec-order-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let ordered = linked_records(&dir, true);
    assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]));
    let broken = linked_records(&dir, false);
    assert!(
        !broken.windows(2).all(|pair| pair[0] <= pair[1]),
        "an unordered emitted section must fail the order check"
    );
    fs::remove_dir_all(dir).unwrap();
}
