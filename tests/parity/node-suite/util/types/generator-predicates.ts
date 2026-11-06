import { types } from "node:util";
import { isGeneratorFunction, isGeneratorObject } from "node:util/types";

function* gf() {
  yield 1;
}
function plain() {}
async function af() {}

const alias = gf;
const gen = gf();
const plainIterator = {
  next() {
    return { done: true, value: undefined };
  },
};

console.log("generator fn:", types.isGeneratorFunction(gf));
console.log("generator alias:", types.isGeneratorFunction(alias));
console.log("plain fn:", types.isGeneratorFunction(plain));
console.log("async fn:", types.isGeneratorFunction(af));
console.log("generator object:", types.isGeneratorObject(gen));
console.log("plain iterator:", types.isGeneratorObject(plainIterator));
console.log("fn object:", types.isGeneratorObject(gf));
console.log("direct fn:", isGeneratorFunction(gf));
console.log("direct object:", isGeneratorObject(gf()));
