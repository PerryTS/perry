const { parentPort } = require("node:worker_threads");
let rows = [];
let seen = 0;
function observe(index, value) {
  if (value === "stop") {
    if (index === 0) parentPort.close();
    return;
  }
  rows[index] = [value === undefined, value === null, value === 7];
  if (++seen === 3) {
    parentPort.postMessage(rows);
    rows = [];
    seen = 0;
  }
}
parentPort.on("message", (value) => observe(0, value));
parentPort.onmessage = (event) => observe(1, event.data);
parentPort.addEventListener("message", (event) => observe(2, event.data));
