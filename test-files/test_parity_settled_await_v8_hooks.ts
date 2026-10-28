import { promiseHooks } from "node:v8";

let before = 0;
let after = 0;
let trash: any;
const stopBefore = promiseHooks.onBefore(() => {
  before++;
  const items: any[] = [];
  for (let j = 0; j < 8; j++) items.push({ value: before + j });
  trash = { phase: "before", items };
});
const stopAfter = promiseHooks.onAfter(() => {
  after++;
  const items: any[] = [];
  for (let j = 0; j < 8; j++) items.push({ value: after + j });
  trash = { phase: "after", items };
});

async function run(): Promise<void> {
  await 0;
  const owner = { key: "object", callback: (n: number) => n + 1 };
  let total = 0;
  for (let i = 0; i < 32; i++) {
    const value = await Promise.resolve(owner);
    if (value !== owner) throw new Error("lost pointer identity");
    total += value.callback(i);
  }
  stopBefore();
  stopAfter();
  console.log("settled hooks", before > 0, after > 0, total, !!trash.items);
}
run();
