// Synchronous child_process option validation reached through cluster.fork.
import cluster from "node:cluster";

for (
  const [name, value] of [["cwd", 1], ["stdio", "bad"], ["uid", "bad"], [
    "gid",
    "bad",
  ]] as const
) {
  cluster.setupPrimary({ [name]: value } as any);
  try {
    cluster.fork();
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
