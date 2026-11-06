// 10,000 setImmediate calls in one turn: Node order, and (WTC17) scheduling
// an Immediate reads no clock: only a timer has a start time.
const order: string[] = [];
let ran = 0;
setTimeout(() => {
  for (let i = 0; i < 10000; i++) {
    setImmediate(() => {
      ran++;
    });
  }
  setImmediate(() => order.push("immediate-last:" + ran));
  process.nextTick(() => order.push("tick"));
  Promise.resolve().then(() => order.push("micro"));
  setImmediate(() => order.push("immediate-after"));
}, 1);
setTimeout(() => console.log(ran, order.join(" ")), 50);
