import test from "node:test";

test("callback completion", (_t, done) => {
  console.log("done:body");
  done();
  console.log("done:after-callback");
});
