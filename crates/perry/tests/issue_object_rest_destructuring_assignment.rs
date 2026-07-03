//! Regression test: object REST in a destructuring *assignment*
//! (`({ a, ...rest } = obj)` — parenthesized assignment, not a `let`/`const`
//! declaration) must bind `rest` to a fresh object of the remaining own
//! properties. Perry previously *skipped* the rest element in the
//! assignment-target lowering (`destructuring/assignment_expr.rs` and
//! `assignment_stmt.rs` both had `ObjectPatProp::Rest(_) => { /* skip */ }`),
//! leaving `rest` `undefined`.
//!
//! This is the shape the React Compiler emits for memoized components: an ink
//! `Box`/`Text` destructures its props via
//!   `({ flexDirection: m, children: x, ...rest } = q)`
//! and forwards `...rest` (which carries the remaining style props) into the
//! ink-box style. With `rest` undefined, every explicit style — crucially
//! `flexDirection` when it lands in the rest — was dropped and the layout
//! collapsed to the yoga default (`flexDirection: "row"`), so column layouts
//! rendered as rows. Node binds the rest correctly; perry must too.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn object_rest_in_destructuring_assignment_binds_remaining_props() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
// (1) statement form: `({ a, ...rest } = obj);`
let m: any, x: any, rest: any;
const props: any = { flexDirection: "column", children: 1, gap: 2, marginTop: 3 };
({ flexDirection: m, children: x, ...rest } = props);
console.log("stmt m:", m);
console.log("stmt rest:", JSON.stringify(rest));

// (2) expression form inside a comma-sequence (the React-Compiler shape:
//     `if (a = x, ({...} = q), cond) { ... }`).
let m2: any, rest2: any, sentinel = 0;
const props2: any = { flexDirection: "column", onClick: 0, gap: 9 };
if ((sentinel = 1, ({ flexDirection: m2, onClick: x, ...rest2 } = props2), rest2.gap === 9)) {
  console.log("expr m2:", m2, "sentinel:", sentinel);
}

// (3) the actual failure mode: flexDirection carried IN the rest, then defaulted.
let rest3: any;
const props3: any = { children: 7, flexDirection: "column" };
({ children: x, ...rest3 } = props3);
const resolved = rest3.flexDirection === void 0 ? "row" : rest3.flexDirection;
console.log("resolved flexDirection:", resolved);
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "stmt m: column\n\
         stmt rest: {\"gap\":2,\"marginTop\":3}\n\
         expr m2: column sentinel: 1\n\
         resolved flexDirection: column\n",
        "object rest in a destructuring assignment must bind the remaining props \
         (node prints the same); a dropped rest is the ink column->row paint bug"
    );
}
