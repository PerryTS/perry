// #3896: reading an absent member off a Node-core module namespace object is an
// ordinary JavaScript property miss → undefined (matching Node), even though the
// named import of that member is rejected. (The call form `ns.foo()` still
// throws/errors as unimplemented — covered by the perry-hir unit tests.)
import * as dnsp from "node:dns/promises";
import * as os from "node:os";
import * as crypto from "node:crypto";
console.log("dns/promises:", typeof (dnsp as any).ADDRCONFIG, typeof (dnsp as any).V4MAPPED, typeof dnsp.lookup);
console.log("os:", typeof (os as any).__definitelyNotAnOsMember__, typeof os.platform);
console.log("crypto:", typeof (crypto as any).__definitelyNotACryptoMember__, typeof crypto.randomUUID);
