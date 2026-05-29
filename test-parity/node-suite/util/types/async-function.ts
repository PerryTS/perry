import { types } from "node:util";
import { isAsyncFunction } from "node:util/types";

async function af() {}
async function awaited() {
  await Promise.resolve(1);
}
function plain() {}
const alias = af;

console.log("async fn:", types.isAsyncFunction(af));
console.log("async awaited:", types.isAsyncFunction(awaited));
console.log("alias:", types.isAsyncFunction(alias));
console.log("plain fn:", types.isAsyncFunction(plain));
console.log("arrow:", types.isAsyncFunction(async () => 1));
console.log("direct:", isAsyncFunction(af));
