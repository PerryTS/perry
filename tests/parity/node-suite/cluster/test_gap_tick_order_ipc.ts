// The callback of process.send (IPC) runs from the tick queue: before a
// setImmediate the program scheduled earlier in the same turn, as in node.
import cluster from "node:cluster";
if (cluster.isPrimary) {
  const w = cluster.fork();
  w.on("message", (m) => {
    console.log(m);
    if (m !== "hello") w.disconnect();
  });
} else {
  const order: string[] = [];
  setImmediate(() => {
    order.push("immediate");
    process.send!(order.join(" "));
  });
  process.send!("hello", () => order.push("callback"));
}
