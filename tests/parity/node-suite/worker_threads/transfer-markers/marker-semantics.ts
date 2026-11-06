import {
  isMarkedAsUntransferable,
  markAsUncloneable,
  markAsUntransferable,
} from "node:worker_threads";

for (const value of [undefined, null, false, 0, "value"]) {
  console.log(
    "primitive:",
    typeof value,
    markAsUntransferable(value),
    isMarkedAsUntransferable(value),
  );
}

const object = {};
const array: any[] = [];
console.log("object before:", isMarkedAsUntransferable(object));
console.log("object mark return:", markAsUntransferable(object));
markAsUntransferable(array);
console.log(
  "objects after:",
  isMarkedAsUntransferable(object),
  isMarkedAsUntransferable(array),
);

const prototype = {};
markAsUntransferable(prototype);
const child = Object.create(prototype);
console.log(
  "not inherited:",
  isMarkedAsUntransferable(prototype),
  isMarkedAsUntransferable(child),
);
console.log("uncloneable return:", markAsUncloneable({}));
