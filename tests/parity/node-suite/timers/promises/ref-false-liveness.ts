import {
  scheduler,
  setImmediate,
  setInterval,
  setTimeout,
} from "node:timers/promises";

setTimeout(1000, "timeout", { ref: false }).then((value) =>
  console.log("timeout resolved:", value)
);
setImmediate("immediate", { ref: false }).then((value) =>
  console.log("immediate resolved:", value)
);
scheduler.wait(1000, { ref: false }).then(() => console.log("wait resolved"));

const interval = setInterval(1000, "interval", { ref: false });
interval.next().then((result) =>
  console.log("interval resolved:", result.value, result.done)
);

console.log("scheduled");
