// #3115/#3116/#3117 — node:cluster primary lifecycle: persistent
// setupPrimary/setupMaster settings, fork returning a Worker handle, and
// disconnect callbacks running after worker shutdown.
import cluster from "node:cluster";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const childSrc = [
  "import cluster from 'node:cluster';",
  "process.send({",
  "  isWorker: cluster.isWorker,",
  "  isPrimary: cluster.isPrimary,",
  "  workerId: cluster.worker.id,",
  "  argv2: process.argv[2],",
  "  env: process.env.PERRY_CLUSTER_PROBE",
  "});",
  "process.on('disconnect', () => process.exit(0));",
  "setInterval(() => {}, 1000);",
].join("\n");

const childPath = path.join(os.tmpdir(), "perry_cluster_child_" + process.pid + ".mjs");
fs.writeFileSync(childPath, childSrc);

const initialSettings = cluster.settings;
console.log("settings stable initially:", initialSettings === cluster.settings);

const setupPrimaryReturn = cluster.setupPrimary({
  exec: childPath,
  execArgv: [],
  args: ["child-arg"],
  silent: true,
  serialization: "json",
});
const primarySettings = cluster.settings;
console.log("setupPrimary return:", setupPrimaryReturn);
console.log("setupPrimary replaced settings:", initialSettings !== primarySettings);
console.log("setupPrimary args:", cluster.settings.args.join(","));
console.log("setupPrimary silent:", cluster.settings.silent);
console.log("setupPrimary serialization:", cluster.settings.serialization);

const setupMasterReturn = cluster.setupMaster({
  execArgv: ["--no-warnings"],
  args: ["master-alias"],
  silent: false,
});
const masterSettings = cluster.settings;
console.log("setupMaster return:", setupMasterReturn);
console.log("setupMaster replaced settings:", primarySettings !== masterSettings);
console.log("setupMaster exec retained:", cluster.settings.exec === childPath);
console.log("setupMaster args:", cluster.settings.args.join(","));
console.log("setupMaster execArgv:", cluster.settings.execArgv.join(","));
console.log("setupMaster silent:", cluster.settings.silent);
console.log("setupMaster serialization retained:", cluster.settings.serialization);

cluster.setupPrimary({
  exec: childPath,
  execArgv: [],
  args: ["child-arg"],
  silent: true,
  serialization: "json",
});

const worker = cluster.fork({ PERRY_CLUSTER_PROBE: "1" });
console.log("fork typeof:", typeof worker);
console.log("worker id:", worker.id);
console.log("worker connected initially:", worker.isConnected());
console.log("worker dead initially:", worker.isDead());
console.log("workers maps worker:", cluster.workers[worker.id] === worker);

let sawOnline = false;
let disconnectSummary = "";
let callbackSummary = "";
let exitSummary = "";
let sawCallback = false;
let sawExit = false;

await new Promise<void>((resolve) => {
  const maybeResolve = () => {
    if (sawCallback && sawExit) resolve();
  };

  worker.on("online", () => {
    sawOnline = true;
  });
  worker.on("message", (m: any) => {
    console.log(
      "message:",
      m.isWorker,
      m.isPrimary,
      m.workerId,
      m.argv2,
      m.env,
      "online:",
      sawOnline,
    );
    const disconnectReturn = cluster.disconnect(() => {
      callbackSummary = [
        Object.keys(cluster.workers).length,
        worker.isDead(),
        worker.isConnected(),
      ].join(",");
      sawCallback = true;
      maybeResolve();
    });
    console.log("disconnect return:", disconnectReturn);
  });
  worker.on("disconnect", () => {
    disconnectSummary = [worker.isConnected(), worker.isDead()].join(",");
  });
  worker.on("exit", (code: number | null, signal: string | null) => {
    exitSummary = [
      code,
      signal,
      worker.isDead(),
      Object.keys(cluster.workers).length,
    ].join(",");
    sawExit = true;
    maybeResolve();
  });
});

console.log("worker disconnect:", disconnectSummary);
console.log("disconnect callback:", callbackSummary);
console.log("worker exit:", exitSummary);

fs.unlinkSync(childPath);
console.log("cluster lifecycle done");
