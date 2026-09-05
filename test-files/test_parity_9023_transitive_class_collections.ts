// Import only helpers: their inlined bodies must not lose the transitive class.
import { makeBag, getBag, track } from "./fixtures/issue_9023/helpers.ts";
const empty = getBag(new Map(), 1);
let count = 0;
for (const pair of empty) count++;
console.log("empty", count);
const bag = makeBag();
bag.add(1, 2);
bag.add(1, 3);
for (const [key, value] of bag) console.log("pair", key, value);
const store = new Map();
track(store, 9, 4, 5);
for (const [key, value] of getBag(store, 9)) console.log("tracked", key, value);
