// SNC 15: awaits inside an async generator keep the ordinary FIFO order.
const log: string[] = [];
async function* gen(): AsyncGenerator<number> {
  for (let i = 0; i < 4; i++) {
    await i;
    yield i;
  }
}
async function drive(): Promise<void> {
  for await (const v of gen()) {
    log.push("g" + v);
    await v;
  }
}
async function other(): Promise<void> {
  for (let i = 0; i < 8; i++) {
    await i;
    log.push("o" + i);
  }
}
Promise.all([drive(), other()]).then(() => console.log(log.join(" ")));
