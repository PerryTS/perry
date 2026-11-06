import test from "node:test";

test("async callback", async () => {
  console.log("async:start");
  await Promise.resolve();
  console.log("async:after-await");
  return "ignored return value";
});
