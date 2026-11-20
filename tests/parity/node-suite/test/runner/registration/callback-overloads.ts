import test from "node:test";

test(() => {
  console.log("overload:callback-only");
});

test(function namedCallback() {
  console.log("overload:named-callback");
});

test({ skip: false }, () => {
  console.log("overload:options-callback");
});

test("name-options-callback", { skip: false }, (t) => {
  console.log("overload:full", t.name);
});
