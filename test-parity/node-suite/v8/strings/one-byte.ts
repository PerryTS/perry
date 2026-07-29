import { isStringOneByteRepresentation } from "node:v8";

for (const value of ["", "abc", "é", "€", "😀"] as const) {
  console.log(
    JSON.stringify(value) + ":",
    isStringOneByteRepresentation(value),
  );
}

for (const value of [undefined, 1, null] as const) {
  try {
    isStringOneByteRepresentation(value as any);
    console.log("invalid " + String(value) + ": no throw");
  } catch (error: any) {
    console.log("invalid " + String(value) + ":", error.name, error.code);
  }
}
