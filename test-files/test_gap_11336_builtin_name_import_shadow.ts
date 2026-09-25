// #11336: a binding imported from a SOURCE module under a builtin module's
// name (`crypto`, `path`, `os`) must shadow the builtin. Perry used to lower
// `crypto.sha256(x)` on such a binding to its own hex-digest intrinsic, which
// broke node-postgres' scram-sha-256 login: pg's lib/crypto/sasl.js binds its
// WebCrypto helper module as `const crypto = require('./utils')`, so the
// StoredKey became a hex string and every SCRAM proof was wrong.
import * as crypto from "./fixtures/issue_11336/fake_builtins.ts";
import * as path from "./fixtures/issue_11336/fake_builtins.ts";
import * as os from "./fixtures/issue_11336/fake_builtins.ts";
import scram from "./fixtures/issue_11336/scram_client.cjs";
import * as realPath from "node:path";
import * as realCrypto from "node:crypto";

console.log("esm crypto:", crypto.sha256("a"), crypto.md5("b"));
console.log("esm path:", path.join("x", "y"), path.basename("/p/q"));
console.log("esm os:", os.hostname(), os.platform());
console.log("cjs other:", scram.otherMethods());

// RFC 7677's SCRAM-SHA-256 exchange, with pg's `n=*` username (pg sends the
// user in the startup packet, not the SCRAM message).
const r = await scram.clientFinal(
  "pencil",
  "rOprNGfwEbeRWgbNEkqO",
  "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096",
);
console.log("storedKey:", r.storedKey);
console.log("client-final:", r.response);
console.log("server-signature:", r.serverSignature);

// The real builtins, bound under other names, still resolve (control).
console.log("control:", realPath.join("a", "b"), realCrypto.createHash("sha256").update("a").digest("hex").slice(0, 16));
