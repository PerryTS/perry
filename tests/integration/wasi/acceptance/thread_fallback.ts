import { spawn } from "perry/thread";
import { collect } from "perry/gc";

const shared = { count: 0 };
const order: string[] = [];
spawn(() => {
  collect();
  shared.count++;
  order.push("task");
  return shared.count;
}).then(value => {
  console.log(order.join(","), value, shared.count);
});
order.push("sync");
spawn(() => { throw new Error("spawn rejected"); }).catch((error: any) => {
  collect();
  console.log(error.message, shared.count);
});
