import zlib from "node:zlib";

function write(target: any, key: string, value: any) {
  try {
    target[key] = value;
    return "assigned";
  } catch (error: any) {
    return error.name;
  }
}

const moduleObject: any = zlib;
const constants: any = zlib.constants;
const aliases: string[] = [];
let constantDescriptors = true;
let constantWrites = true;
let aliasWrites = true;
for (const key of Object.keys(constants)) {
  const value = constants[key];
  const descriptor = Object.getOwnPropertyDescriptor(constants, key)!;
  constantDescriptors = constantDescriptors && descriptor.value === value &&
    descriptor.enumerable && !descriptor.writable && !descriptor.configurable;
  constantWrites = constantWrites && write(constants, key, value + 1) === "TypeError" &&
    constants[key] === value;
  const alias = Object.getOwnPropertyDescriptor(moduleObject, key);
  if (alias !== undefined) {
    aliases.push(key);
    console.log("alias:", key, alias.value === value, alias.enumerable,
      alias.writable, alias.configurable);
    aliasWrites = aliasWrites && write(moduleObject, key, value + 1) === "TypeError" &&
      moduleObject[key] === value;
  }
}
console.log("constant descriptors:", constantDescriptors);
console.log("constant writes:", constantWrites);
console.log("alias writes:", aliasWrites);
console.log("aliases enumerable:", Object.keys(moduleObject).some((key) => aliases.includes(key)));
for (const key of ["codes", "constants"]) {
  const value = moduleObject[key];
  const descriptor = Object.getOwnPropertyDescriptor(moduleObject, key)!;
  console.log("export:", key, descriptor.value === value, descriptor.enumerable,
    descriptor.writable, descriptor.configurable);
  console.log("export write:", key, write(moduleObject, key, {}), moduleObject[key] === value);
}
console.log("codes frozen:", Object.isFrozen(zlib.codes));
console.log("codes write:", write(zlib.codes, "Z_OK", 123), zlib.codes.Z_OK);
const key = "gzipSync";
const original = moduleObject[key];
function replacement() { return "replacement"; }
console.log("mutable function write:", write(moduleObject, key, replacement));
console.log("mutable function identity:", moduleObject[key] === replacement);
console.log("mutable function restore:", write(moduleObject, key, original));
console.log("restored identity:", moduleObject[key] === original);
