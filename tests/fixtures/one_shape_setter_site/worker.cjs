const { parentPort } = require("node:worker_threads");
class WorkerMeter {
  constructor() { this.value = 0; this.writes = 0; }
  get points() { return this.value; }
  set points(next) { this.value = next; this.writes++; }
}
const meter = new WorkerMeter();
function write(target, next) { target.points = next; }
parentPort.on("message", (message) => {
  if (message !== "go") return;
  let churn = [];
  for (let i = 0; i < 20000; i++) churn.push({ i });
  global.gc();
  churn = [];
  for (let i = 0; i < 1000; i++) write(meter, i % 17);
  parentPort.postMessage(`${meter.points} ${meter.writes}`);
});
parentPort.postMessage("ready");
