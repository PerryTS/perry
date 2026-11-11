// A suspended top-level await resumed by a callback-style fs completion: the
// sibling evaluates while it waits, and the entry's own await settles too.
import { readdir } from "node:fs";
import { files } from "./fixtures/tla_order/poll-phase/m.ts";
import "./fixtures/tla_order/poll-phase/n.ts";
console.log("entry", files);
const count = await new Promise<number>((resolve) => readdir(".", (_error, entries) => resolve(entries.length)));
console.log("entry done", count > 0);
