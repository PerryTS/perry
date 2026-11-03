// #6287: when several timers come due in the same event-loop turn, the batch
// must fire in event-loop order, not queue (creation) order:
//   1. expired timeouts by DEADLINE (a 20 ms timer created after a 30 ms one
//      still fires first), with same-deadline ties in creation order;
//   2. timeouts (timers phase) before setImmediate (check phase), with
//      immediates FIFO among themselves.
// Each block blocks the loop first, so every timer below is already expired
// when the loop next ticks — that is what makes batch ordering observable.
// Every pair whose deadline order differs from creation order is at least
// 10 ms apart in deadline, so a runner stall of under 10 ms between two
// setTimeout calls cannot flip the expected order.

const log: string[] = [];

// --- deadline ordering, created in reverse-deadline order ---
setTimeout(() => log.push("t30"), 30);
setTimeout(() => log.push("t20"), 20);
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
while (Date.now() - start < 45) {
  /* spin */
}

setTimeout(() => {
  console.log("order:", log.join(","));
  console.log("count:", log.length);
  console.log("no-cancelled:", !log.includes("CANCELLED"));
  console.log("done");
}, 80);
