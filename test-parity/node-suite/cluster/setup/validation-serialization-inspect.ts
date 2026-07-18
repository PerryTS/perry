// Node validates serialization mode and a null inspect port before spawn.
import cluster from "node:cluster";

for (
  const [name, options] of [
    ["serialization", { serialization: "bad" }],
    ["inspectPort", { inspectPort: null }],
  ] as const
) {
  // setupPrimary merges into cluster.settings, so restore valid defaults each
  // iteration and spread the invalid field last; otherwise the previous case's bad
  // value (e.g. serialization: "bad") leaks in and masks this case's validation.
  cluster.setupPrimary({ serialization: "json", inspectPort: undefined, ...options } as any);
  try {
    cluster.fork();
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
