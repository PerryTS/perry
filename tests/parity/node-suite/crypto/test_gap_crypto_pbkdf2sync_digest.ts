import { pbkdf2Sync } from "node:crypto";

// This program has no `require` and no CommonJS interop, so the calls below
// take Perry's static `crypto.pbkdf2Sync` lowering instead of the dynamic
// dispatcher. Node rejects a call without a digest, and the static lowering
// must do the same: forwarding a null digest made it derive a SHA-256 key.
const notAString: any = 256;

console.log("sha256", pbkdf2Sync("password", "salt", 1, 16, "sha256").toString("hex"));
console.log("sha-256", pbkdf2Sync("password", "salt", 1, 16, "sha-256").toString("hex"));
console.log("md5", pbkdf2Sync("password", "salt", 1, 16, "md5").toString("hex"));
console.log("sha512-256", pbkdf2Sync("password", "salt", 1, 16, "sha512-256").toString("hex"));

try {
  pbkdf2Sync("password", "salt", 1, 16);
  console.log("digest omitted no throw");
} catch (error: any) {
  console.log("digest omitted", error.code);
}

try {
  pbkdf2Sync("password", "salt", 1, 16, notAString);
  console.log("digest not a string no throw");
} catch (error: any) {
  console.log("digest not a string", error.code);
}

try {
  pbkdf2Sync("password", "salt", 0, 16, "sha256");
  console.log("iterations 0 no throw");
} catch (error: any) {
  console.log("iterations 0", error.code);
}

// A name Perry does not implement is an error, not a SHA-256 key.
try {
  pbkdf2Sync("password", "salt", 1, 16, "not-a-digest");
  console.log("digest unknown no throw");
} catch (error: any) {
  console.log("digest unknown", error.code);
}

try {
  pbkdf2Sync("password", "salt", 1, 16, "");
  console.log("digest empty no throw");
} catch (error: any) {
  console.log("digest empty", error.code);
}

// Node accepts only OpenSSL's registered spellings; a name that merely looks
// like one after deleting hyphens is an error, not an alias.
try {
  pbkdf2Sync("password", "salt", 1, 16, "m-d5");
  console.log("digest misspelled no throw");
} catch (error: any) {
  console.log("digest misspelled", error.code);
}
