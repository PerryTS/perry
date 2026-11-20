import { text } from "node:stream/consumers";

async function* chunks() {
  yield "a";
  await Promise.resolve();
  yield "b";
}

console.log("text:", await text(chunks()));
