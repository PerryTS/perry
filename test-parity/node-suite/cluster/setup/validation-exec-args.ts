// Node primary.js delegates exec/args validation to child_process.fork before
// creating a child, so these failures are synchronous and leak no process.
import cluster from "node:cluster";

for (const [name, value] of [["exec", 1], ["args", 1]] as const) {
  cluster.setupPrimary({ [name]: value } as any);
  try {
    cluster.fork();
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
