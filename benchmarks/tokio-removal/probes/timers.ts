const order: string[] = [];
setTimeout(() => {
  order.push("t0");
  setImmediate(() => order.push("imm"));
  setTimeout(() => order.push("t0b"), 0);
}, 0);
setTimeout(() => order.push("t20"), 20);
Promise.resolve().then(() => order.push("micro"));
let n = 0;
const iv = setInterval(() => {
  order.push("iv");
  if (++n === 3) clearInterval(iv);
}, 5);
setTimeout(() => console.log(order.join(",")), 80);
