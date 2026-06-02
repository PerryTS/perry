// Behavioral parity test for the uuid package (perry-stdlib).
//
// uuid generators are non-deterministic — assert *shape* and validate
// invariants (length, version nibble, validate() round-trip) rather
// than concrete output.

import { v1 as uuidv1, v4 as uuidv4, v7 as uuidv7, validate as uuidValidate, version as uuidVersion, NIL as uuidNil } from "uuid";

function isUuidShape(s: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s);
}

const v1 = uuidv1();
console.log("v1 length:", v1.length);
console.log("v1 shape ok:", isUuidShape(v1));
console.log("v1 version:", uuidVersion(v1));
console.log("v1 validates:", uuidValidate(v1));

const v4a = uuidv4();
const v4b = uuidv4();
console.log("v4 length:", v4a.length);
console.log("v4 shape ok:", isUuidShape(v4a));
console.log("v4 version:", uuidVersion(v4a));
console.log("v4 validates:", uuidValidate(v4a));
console.log("v4 distinct:", v4a !== v4b);

const v7 = uuidv7();
console.log("v7 length:", v7.length);
console.log("v7 shape ok:", isUuidShape(v7));
console.log("v7 version:", uuidVersion(v7));
console.log("v7 validates:", uuidValidate(v7));

// NIL UUID is a stable, deterministic constant.
console.log("NIL:", uuidNil);
console.log("NIL validates:", uuidValidate(uuidNil));

console.log("validate_bogus:", uuidValidate("not-a-uuid"));

/*
@covers
crates/perry-stdlib/src/uuid.rs:
  - js_uuid_nil
  - js_uuid_v1
  - js_uuid_v4
  - js_uuid_v7
  - js_uuid_validate
  - js_uuid_version
*/
