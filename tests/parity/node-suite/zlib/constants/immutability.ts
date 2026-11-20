import zlib from "node:zlib";

function assignmentResult(target: any, key: string, value: any) {
  try {
    target[key] = value;
    return "assigned";
  } catch (error: any) {
    return `${error.name}:${error.code}`;
  }
}

console.log("codes frozen:", Object.isFrozen(zlib.codes));
console.log("codes value write:", assignmentResult(zlib.codes, "Z_OK", 1));
console.log("codes export write:", assignmentResult(zlib, "codes", {}));
console.log(
  "constant value write:",
  assignmentResult(zlib.constants, "Z_OK", 1),
);
console.log("values unchanged:", zlib.codes.Z_OK, zlib.constants.Z_OK);
