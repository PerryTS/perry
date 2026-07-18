// Node primary.js delegates exec/args validation to child_process.fork before
// creating a child, so these failures are synchronous and leak no process.
import cluster from "node:cluster";

for (const [name, value] of [["exec", 1], ["args", 1]] as const) {
  // setupPrimary merges into cluster.settings, so restore valid defaults for both
  // fields each iteration and override only the current one with the invalid value;
  // otherwise the previous iteration's bad field leaks in and masks this case.
  cluster.setupPrimary({ exec: process.argv[1], args: [], [name]: value } as any);
  try {
    cluster.fork();
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
