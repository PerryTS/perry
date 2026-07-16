// Node validates serialization mode and a null inspect port before spawn.
import cluster from "node:cluster";

for (
  const [name, options] of [
    ["serialization", { serialization: "bad" }],
    ["inspectPort", { inspectPort: null }],
  ] as const
) {
  cluster.setupPrimary(options as any);
  try {
    cluster.fork();
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
