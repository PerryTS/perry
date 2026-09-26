import { Worker } from "node:worker_threads";
const w = new Worker("./worker_child.ts", { workerData: { n: 21 } });
w.on("message", (m) => console.log("main got", m));
w.on("exit", (c) => console.log("worker exit", c));
