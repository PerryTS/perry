// #6287: when several timers come due in the same event-loop turn, the batch
// must fire in event-loop order, not queue (creation) order:
//   1. expired timeouts by DEADLINE (a 5 ms timer created after a 10 ms one
//      still fires first), with same-deadline ties in creation order;
//   2. timeouts (timers phase) before setImmediate (check phase), with
//      immediates FIFO among themselves.
// Each block blocks the loop first, so every timer below is already expired
// when the loop next ticks — that is what makes batch ordering observable.

const log: string[] = [];

// --- deadline ordering, created in reverse-deadline order ---
setTimeout(() => log.push("t10"), 10);
setTimeout(() => log.push("t5"), 5);
setTimeout(() => log.push("t1"), 1);

// --- same deadline => creation order (must not regress) ---
setTimeout(() => log.push("same-a"), 3);
setTimeout(() => log.push("same-b"), 3);
setTimeout(() => log.push("same-c"), 3);

// --- cleared timers never fire, and don't disturb the survivors ---
const cancelled = setTimeout(() => log.push("CANCELLED"), 2);
clearTimeout(cancelled);

// --- immediates: scheduled before/after a timeout, must run after it ---
setImmediate(() => log.push("imm1"));
setTimeout(() => log.push("t7"), 7);
setImmediate(() => log.push("imm2"));

// Block past every deadline above so they all land in one expired batch.
const start = Date.now();
while (Date.now() - start < 30) {
  /* spin */
}

setTimeout(() => {
  console.log("order:", log.join(","));
  console.log("count:", log.length);
  console.log("no-cancelled:", !log.includes("CANCELLED"));
  console.log("done");
}, 50);
