import { deepEqual, notDeepEqual } from "node:assert/strict";

deepEqual("a", "a");
notDeepEqual("a", "b");
console.log("strict named deep imports ok");
