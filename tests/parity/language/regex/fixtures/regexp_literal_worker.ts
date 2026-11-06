import { parentPort } from "node:worker_threads";
import { makeLiteral } from "./regexp_literal_site.ts";
const a = makeLiteral();
a.test("aa");
const b = makeLiteral();
parentPort!.postMessage([a === b, a.lastIndex, b.lastIndex, a.source, b.source]);
