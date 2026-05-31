import test, {
  after,
  afterEach,
  before,
  beforeEach,
  describe,
  it,
  mock,
  run,
  snapshot,
  test as namedTest,
} from "node:test";
import * as reporters from "node:test/reporters";

console.log("test default:", typeof test);
console.log("test identity:", test === namedTest ? "same" : "different");
console.log(
  "mock timers:",
  typeof mock,
  typeof mock.timers,
  typeof mock.timers.enable,
  typeof mock.timers.reset,
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
