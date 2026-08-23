//! Regression for #8654: direct updates of an imported class static must use
//! the same initialized storage as reads in the defining and importing modules.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

const EXPECTED: &str = "primary:ok\nreads:ok\necs:ok\n";

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command
            .output()
            .expect("run cargo build of static runtime archives");
        assert!(
            build.status.success(),
            "cargo build of static runtime archives failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

fn write_fixture(dir: &Path) {
    std::fs::write(
        dir.join("base.ts"),
        r#"
export class Component {
  static _id = 0;
  static anchor = { label: "alive" };
}

export function makeComponent(constructor: any) {
  constructor.id = Component._id++;
}

export function readDefiningModule() {
  return Component._id + ":" + Component.anchor.label;
}

export abstract class EcsComponent {
  static readonly _id: number = 0;
  static readonly id: number;
}

export function registerComponent(constructor: any) {
  constructor.id = (<any>EcsComponent)._id++;
}
"#,
    )
    .expect("write base fixture");

    std::fs::write(
        dir.join("main.ts"),
        r#"
import {
  Component,
  EcsComponent,
  makeComponent,
  readDefiningModule,
  registerComponent,
} from "./base";

class First extends Component {}
class Second extends Component {}

makeComponent(First);                    // defining-module post-increment
const importedOld = Component._id++;     // importing-module post-increment

let keep: any[] = [];
for (let i = 0; i < 12000; i++) {
  const value = { i, pad: "x" + i };
  if (i % 997 === 0) keep.push(value);
}
(globalThis as any).gc?.();

makeComponent(Second);                   // defining-module update after GC
const DynamicComponent: any = Component;
console.log(
  (First as any).id === 0 && importedOld === 1 &&
  (Second as any).id === 2 && Component._id === 3
    ? "primary:ok"
    : "primary:bad",
);
console.log(
  readDefiningModule() === "3:alive" &&
  Component._id === 3 && Component.anchor.label === "alive" &&
  DynamicComponent._id === 3
    ? "reads:ok"
    : "reads:bad",
);

// The component-registration shape used by perform-ecs: sequential class IDs
// become independent bit masks, and entities leave the matching view again.
class Position extends EcsComponent {}
class Velocity extends EcsComponent {}
registerComponent(Position);
registerComponent(Velocity);
const ids = [(Position as any).id, (Velocity as any).id];
const masks = [1 << ids[0], 1 << ids[1]];
const required = masks[0] | masks[1];
const retained: any[] = [];
for (let i = 0; i < 128; i++) {
  const entity = { mask: 0 };
  entity.mask |= masks[0];
  entity.mask |= masks[1];
  if ((entity.mask & required) === required) retained.push(entity);
  entity.mask = 0;
  if ((entity.mask & required) !== required) {
    const index = retained.indexOf(entity);
    if (index >= 0) retained.splice(index, 1);
  }
}
console.log(
  ids[0] === 0 && ids[1] === 1 &&
  masks[0] === 1 && masks[1] === 2 && retained.length === 0
    ? "ecs:ok"
    : "ecs:bad",
);
"#,
    )
    .expect("write main fixture");
}

fn compile(dir: &Path, label: &str, prebuilt_runtime: bool) -> PathBuf {
    let output = dir.join(format!("main_{label}"));
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg("main.ts")
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env_remove("PERRY_NO_AUTO_OPTIMIZE")
        .env_remove("PERRY_RUNTIME_DIR");
    remove_gc_env_overrides(&mut command);
    if prebuilt_runtime {
        ensure_runtime_archive();
        command
            .arg("--no-auto-optimize")
            .env("PERRY_RUNTIME_DIR", target_debug_dir());
    }
    let compile = command.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "{label} compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(binary: &Path, dir: &Path, label: &str, force_evacuation: bool) {
    let mut command = Command::new(binary);
    command.current_dir(dir);
    remove_gc_env_overrides(&mut command);
    if force_evacuation {
        command
            .env("PERRY_GC_SCAVENGE", "1")
            .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_INCREMENTAL", "0");
    }
    let run = command.output().expect("run compiled fixture");
    assert!(
        run.status.success(),
        "{label} run (forced evacuation: {force_evacuation}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        EXPECTED,
        "{label} output differed (forced evacuation: {force_evacuation})"
    );
}

#[test]
fn imported_static_post_increment_shares_one_cell_in_all_build_modes() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture(dir.path());

    for (label, prebuilt_runtime) in [("prebuilt", true), ("auto", false)] {
        let binary = compile(dir.path(), label, prebuilt_runtime);
        run(&binary, dir.path(), label, false);
        run(&binary, dir.path(), label, true);
    }
}
