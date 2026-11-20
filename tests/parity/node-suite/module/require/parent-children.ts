import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const parentPath = req.resolve("./fixtures/parent.cjs");
const childPath = req.resolve("./fixtures/child.cjs");
delete req.cache[parentPath];
delete req.cache[childPath];
const parent = req("./fixtures/parent.cjs");
const parentRecord = req.cache[parentPath]!;
const childRecord = req.cache[childPath]!;
console.log(
  "loaded states:",
  parent.loadedBefore,
  parentRecord.loaded,
  childRecord.loaded,
);
console.log(
  "children:",
  parent.children.length,
  parent.children[0] === childPath,
  parentRecord.children[0] === childRecord,
);
console.log(
  "parent:",
  childRecord.parent === parentRecord,
  parent.child.parentId === parentPath,
);
req("./fixtures/child.cjs");
console.log("no duplicate child:", parentRecord.children.length);
delete req.cache[parentPath];
delete req.cache[childPath];
