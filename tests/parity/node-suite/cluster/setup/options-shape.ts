// Observable setup settings cover the child_process.fork options forwarded by
// Node primary.js without actually launching a worker.
import cluster from "node:cluster";

const options: any = {
  exec: "fixture.js",
  args: ["one", "two"],
  execArgv: ["--no-warnings"],
  cwd: ".",
  silent: true,
  serialization: "advanced",
  inspectPort: 0,
  windowsHide: true,
  env: { LOCAL: "yes" },
};
cluster.setupPrimary(options);
for (const key of Object.keys(options).sort()) {
  const value = (cluster.settings as any)[key];
  console.log(key, typeof value, Array.isArray(value), value === options[key]);
}
