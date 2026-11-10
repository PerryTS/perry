import test, {
  after,
  afterEach,
  before,
  beforeEach,
  describe,
  it,
  mock,
  only,
  run,
  skip,
  snapshot,
  suite,
  test as namedTest,
  todo,
} from "node:test";
import * as reporters from "node:test/reporters";

console.log("test default:", typeof test);
console.log("test identity:", test === namedTest ? "same" : "different");
console.log(
  "test controls:",
  [typeof skip, typeof todo, typeof only, typeof suite].join(","),
);
console.log(
  "test methods:",
  [typeof test.skip, typeof test.todo, typeof test.only].join(","),
);
console.log(
  "mock timers:",
  typeof mock,
  typeof mock.timers,
  typeof mock.timers.enable,
  typeof mock.timers.reset,
);
console.log(
  "mock tracker:",
  [
    typeof mock.fn,
    typeof mock.method,
    typeof mock.property,
    typeof mock.restoreAll,
  ].join(","),
);
console.log(
  "snapshot helpers:",
  typeof snapshot.setDefaultSnapshotSerializers,
  typeof snapshot.setResolveSnapshotPath,
);
console.log(
  "registration helpers:",
  [
    typeof describe,
    typeof it,
    typeof before,
    typeof after,
    typeof beforeEach,
    typeof afterEach,
  ].join(","),
);
console.log("run:", typeof run);
console.log(
  "reporters:",
  ["spec", "tap", "dot", "junit", "lcov"]
    .map((name) => typeof (reporters as any)[name])
    .join(","),
);
console.log("reporters default:", typeof (reporters as any).default);
