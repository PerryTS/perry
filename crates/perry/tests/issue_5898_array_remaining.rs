//! Regression coverage for the final built-ins/Array clusters in #5898.
//!
//! DEC 250 (Q2): a program adds no key to `Function.prototype` or
//! `Object.prototype` and no index key to `Array.prototype`. The inherited
//! members these cases read therefore live on a prototype the program
//! inserts into the chain (`Object.setPrototypeOf`), which keeps every case's
//! read through the chain; the refused adds are asserted at the end.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn remaining_array_exotic_and_large_length_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    let runtime_dir = perry_bin()
        .parent()
        .expect("perry binary directory")
        .to_path_buf();
    std::fs::write(
        &entry,
        r#"
const ctorProto: any = Object.create(Function.prototype);
Object.setPrototypeOf(Array, ctorProto);
ctorProto.myproperty = 1;
console.log("ctor-proto", (Array as any).myproperty, Array.hasOwnProperty("myproperty"));

Object.defineProperty(ctorProto, "1", {
  get() { return 6.99; },
  configurable: true
});
const ctorIndex = (Array as any)[1];
Object.setPrototypeOf(Array, Function.prototype);
console.log("ctor-index", ctorIndex, (Array as any)[1], (Array as any).myproperty);

console.log("huge", new Array(4294967295).length);
console.log(
  "of",
  Array.isArray(Array.of.call(undefined)),
  Array.isArray(Array.of.call(Math.cos)),
  Array.isArray(Array.of.call(Math.cos.bind(Math)))
);
console.log(
  "flat",
  Array.prototype.flat.call(true).length,
  Array.prototype.flat.call(false).length
);

let genericPushThrew = false;
const generic: any = { length: Infinity };
try { Array.prototype.push.call(generic, "x"); } catch (e) {
  genericPushThrew = e instanceof TypeError;
}
console.log("generic-push", genericPushThrew, generic[9007199254740991] === undefined);

const max: any[] = [];
max.length = 4294967295;
console.log("max-empty-push", max.push());
let maxPushThrew = false;
try { max.push("x"); } catch (e) { maxPushThrew = e instanceof RangeError; }
console.log("max-push", maxPushThrew, max[4294967295], max.length);

// An index accessor on the array's own prototype (DEC 250: not on
// Array.prototype itself).
function indexedProto(): any {
  return Object.create(Array.prototype);
}

let freezeCalls = 0;
const frozenDuringPush: any[] = [];
const pushFreezeProto = indexedProto();
Object.setPrototypeOf(frozenDuringPush, pushFreezeProto);
Object.defineProperty(pushFreezeProto, "0", {
  set(_value) { freezeCalls++; Object.freeze(frozenDuringPush); },
  configurable: true
});
let freezePushThrew = false;
try { frozenDuringPush.push(1); } catch (e) { freezePushThrew = e instanceof TypeError; }
console.log(
  "push-freeze",
  freezePushThrew,
  freezeCalls,
  frozenDuringPush.hasOwnProperty(0),
  frozenDuringPush.length
);

let lengthCalls = 0;
const lockedDuringPush: any[] = [];
const pushLengthProto = indexedProto();
Object.setPrototypeOf(lockedDuringPush, pushLengthProto);
Object.defineProperty(pushLengthProto, "0", {
  set(_value) {
    lengthCalls++;
    Object.defineProperty(lockedDuringPush, "length", { writable: false });
  },
  configurable: true
});
let lockedPushThrew = false;
try { lockedDuringPush.push(1); } catch (e) { lockedPushThrew = e instanceof TypeError; }
console.log(
  "push-length",
  lockedPushThrew,
  lengthCalls,
  lockedDuringPush.hasOwnProperty(0),
  lockedDuringPush.length
);

let visible = false;
let reduceRightOk = false;
let reduceRightTrace = "missing";
const reduced: any[] = [0];
Object.defineProperty(reduced, "1", {
  get() { return visible ? 1 : "20"; },
  configurable: true
});
Object.defineProperty(reduced, "2", {
  get() { visible = true; return 2; },
  configurable: true
});
reduced.reduceRight((previous, current, index) => {
  if (index === 1) {
    reduceRightTrace = previous + ":" + current;
    reduceRightOk = current === 1 && previous === 2;
  }
});
console.log("reduce-right", reduceRightOk, reduceRightTrace, visible);

const shiftedProto = indexedProto();
shiftedProto[0] = 1;
const shifted: any[] = [];
Object.setPrototypeOf(shifted, shiftedProto);
shifted.length = 1;
console.log("unshift-array", shifted.unshift(0), shifted[0], shifted[1]);
delete shifted[0];
console.log("unshift-array-proto", shifted[0]);

const genericProto: any = {};
genericProto[0] = 1;
genericProto.length = 1;
genericProto.unshift = Array.prototype.unshift;
let shiftedObject = [9];
shiftedObject = Object.create(genericProto);
console.log(
  "unshift-object",
  shiftedObject.unshift(0),
  shiftedObject[0],
  shiftedObject[1],
  shiftedObject.length
);
delete shiftedObject[0];
delete shiftedObject.length;
console.log("unshift-object-proto", shiftedObject[0], shiftedObject.length);
let multiObject = [9];
multiObject = Object.create(genericProto);
console.log(
  "unshift-object-multi",
  multiObject.unshift(7, 8),
  multiObject[0],
  multiObject[1],
  multiObject[2],
  multiObject.length
);

const joinProto: any = {};
joinProto[1] = 1;
joinProto.length = 2;
joinProto.join = Array.prototype.join;
let joinedObject = [9];
const joinSource: any = Object.create(joinProto);
joinSource[0] = 0;
joinedObject = joinSource;
console.log("join-object", joinedObject.join());

let unshiftSetterCalls = 0;
const frozenDuringUnshift: any[] = [];
const unshiftFreezeProto = indexedProto();
Object.setPrototypeOf(frozenDuringUnshift, unshiftFreezeProto);
Object.defineProperty(unshiftFreezeProto, "0", {
  set(_value) { unshiftSetterCalls++; Object.freeze(frozenDuringUnshift); },
  configurable: true
});
let freezeUnshiftThrew = false;
try { frozenDuringUnshift.unshift(1); } catch (e) {
  freezeUnshiftThrew = e instanceof TypeError;
}
console.log(
  "unshift-freeze",
  freezeUnshiftThrew,
  unshiftSetterCalls,
  frozenDuringUnshift.hasOwnProperty(0),
  frozenDuringUnshift.length
);

let unshiftLengthCalls = 0;
const lockedDuringUnshift: any[] = [];
const unshiftLengthProto = indexedProto();
Object.setPrototypeOf(lockedDuringUnshift, unshiftLengthProto);
Object.defineProperty(unshiftLengthProto, "0", {
  set(_value) {
    unshiftLengthCalls++;
    Object.defineProperty(lockedDuringUnshift, "length", { writable: false });
  },
  configurable: true
});
let lockedUnshiftThrew = false;
try { lockedDuringUnshift.unshift(1); } catch (e) {
  lockedUnshiftThrew = e instanceof TypeError;
}
console.log(
  "unshift-length",
  lockedUnshiftThrew,
  unshiftLengthCalls,
  lockedDuringUnshift.hasOwnProperty(0),
  lockedDuringUnshift.length
);

// The adds the original cases made are refused and change nothing.
function refused(add: () => void): boolean {
  try { add(); return false; } catch (e) { return e instanceof TypeError; }
}
console.log(
  "refused",
  refused(() => { (Function.prototype as any).myproperty = 1; }),
  refused(() => { (Array.prototype as any)[0] = 1; }),
  refused(() => { Object.defineProperty(Object.prototype, "1", { get() { return 6.99; }, configurable: true }); }),
  (Array as any).myproperty,
  ([] as any)[0],
  (Array as any)[1]
);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_LIB_DIR", &runtime_dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "ctor-proto 1 false\n",
            "ctor-index 6.99 undefined undefined\n",
            "huge 4294967295\n",
            "of true true true\n",
            "flat 0 0\n",
            "generic-push true true\n",
            "max-empty-push 4294967295\n",
            "max-push true x 4294967295\n",
            "push-freeze true 1 false 0\n",
            "push-length true 1 false 0\n",
            "reduce-right true 2:1 true\n",
            "unshift-array 2 0 1\n",
            "unshift-array-proto 1\n",
            "unshift-object 2 0 1 2\n",
            "unshift-object-proto 1 1\n",
            "unshift-object-multi 3 7 8 1 3\n",
            "join-object 0,1\n",
            "unshift-freeze true 1 false 0\n",
            "unshift-length true 1 false 0\n",
            "refused true true true undefined undefined undefined\n"
        )
    );
}
