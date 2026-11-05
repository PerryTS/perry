import { crc32 } from "node:zlib";

console.log("negative zero:", crc32("abc", -0) === crc32("abc", 0));
for (const value of [0, 0xffffffff, -1, 0x100000000, 1.5, "0"] as any[]) {
  try {
    console.log(String(value), crc32("abc", value));
  } catch (error: any) {
    console.log(String(value), error.name, error.code);
  }
}
