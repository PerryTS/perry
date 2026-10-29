// SNC 7: pending await first (thunk-dispatched resume), then fused awaits.
const log: string[] = [];
async function f(): Promise<void> {
  const gate = new Promise<number>((r) => setTimeout(() => r(7), 1));
  log.push("g" + (await gate));
  for (let i = 0; i < 5; i++) {
    await i;
    log.push("a" + i);
  }
  const later = new Promise<number>((r) => queueMicrotask(() => r(9)));
  log.push("l" + (await later));
  for (let i = 0; i < 3; i++) {
    await i;
    log.push("b" + i);
  }
}
f().then(() => console.log(log.join(" ")));
Promise.resolve().then(() => log.push("p"));
