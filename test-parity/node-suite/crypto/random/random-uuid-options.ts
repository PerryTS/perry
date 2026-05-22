import * as crypto from "node:crypto";

const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const a = crypto.randomUUID({ disableEntropyCache: true });
const b = crypto.randomUUID({ disableEntropyCache: true });
console.log("uuid option shape a:", re.test(a));
console.log("uuid option shape b:", re.test(b));
console.log("uuid option unique:", a !== b);
console.log("uuid option type:", typeof a);
