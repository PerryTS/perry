//! End-to-end regression coverage for #9371. Above one million elements,
//! `new Array(n)` deliberately starts with a small backing store. Sequential
//! indexed writes must materialize dense storage without losing earlier
//! values or falling into quadratic string-keyed property insertion.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn large_presized_arrays_fill_densely_and_preserve_every_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;

function fillAndVerify(slots: number, addExpando: boolean): string {
  const values: number[] = new Array(slots);
  if (addExpando) (values as any).label = "kept";
  for (let i = 0; i < slots; i++) values[i] = i + 0.25;
  if (addExpando) gc();

  let wrong = 0;
  for (let i = 0; i < slots; i++) {
    if (values[i] !== i + 0.25) wrong++;
  }
  return `${slots}:${wrong}:${values[0]}:${values[slots - 1]}:${(values as any).label}`;
}

console.log(fillAndVerify(900000, false));
console.log(fillAndVerify(1000001, false));
console.log(fillAndVerify(1200000, true));

interface Cell { value: number }
const cells: Cell[] = new Array(1000001);
for (let i = 0; i < 256; i++) cells[i] = { value: i };
gc();
let cellSum = 0;
for (let i = 0; i < 256; i++) cellSum += cells[i].value;
console.log("cells", cells.length, cellSum, cells[0].value, cells[255].value);

const huge: number[] = new Array(4294967295);
huge[0] = 7;
huge[16] = 9;
huge[100000000] = 11;
console.log(huge.length, huge[0], huge[1] === undefined, huge[16], huge[100000000]);
"#,
    )
    .expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let expected = "900000:0:0.25:899999.25:undefined\n\
                    1000001:0:0.25:1000000.25:undefined\n\
                    1200000:0:0.25:1199999.25:kept\n\
                    cells 1000001 32640 0 255\n\
                    4294967295 7 true 9 11\n";
    for moving_gc in [false, true] {
        let mut command = Command::new(&output);
        if moving_gc {
            command
                .env("PERRY_GC_FORCE_EVACUATE", "1")
                .env("PERRY_GC_VERIFY_EVACUATION", "1");
        }
        let run = command.output().expect("run compiled fixture");
        assert!(
            run.status.success(),
            "compiled fixture failed with moving_gc={moving_gc}\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
            run.status,
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    }
}
