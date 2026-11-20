import { Readable, isDisturbed } from "node:stream";
// isDisturbed is false on a fresh, unread stream.
const r = new Readable({ read() {} });
console.log("isDisturbed:", isDisturbed(r));
console.log("is false:", isDisturbed(r) === false);
