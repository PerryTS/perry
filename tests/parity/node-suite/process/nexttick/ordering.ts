// process.nextTick(cb) defers cb until after the current synchronous run.
let log: string[] = [];
process.nextTick(() => log.push("tick"));
log.push("sync");
process.nextTick(() => {
  log.push("tick2");
  console.log("order:", log.join(","));
});
