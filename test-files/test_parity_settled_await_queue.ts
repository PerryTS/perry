const events: string[] = [];

async function bounce(value: any): Promise<any> {
  await 0;
  return await Promise.resolve(value);
}

async function run(): Promise<void> {
  const value = { key: "pointer", callback: (n: number) => n + 1 };
  events.push("start");
  await 0;
  events.push("first");
  queueMicrotask(() => events.push("prior-job"));
  const received = await Promise.resolve(value);
  const scratch: any[] = [];
  for (let j = 0; j < 8; j++) scratch.push({ value: j });
  if (scratch[7].value !== 7) throw new Error("lost loop data");
  events.push(received === value && received.callback(4) === 5 ? "pointer" : "BAD");
  const bounced = await bounce(value);
  events.push(bounced === value ? "owned" : "BAD");
  try {
    await Promise.reject(value);
  } catch (error) {
    events.push(error === value ? "caught" : "BAD");
  } finally {
    events.push("finally");
  }
  const thenable = {
    get then() {
      events.push("get-then");
      return (resolve: (v: any) => void) => {
        events.push("call-then");
        resolve(value);
      };
    },
  };
  const assimilated = await thenable;
  events.push(assimilated === value ? "assimilated" : "BAD");
  const gate = Promise.withResolvers<any>();
  async function wait(): Promise<any> {
    await 0;
    return await gate.promise;
  }
  const waiting = wait();
  queueMicrotask(() => gate.resolve(value));
  const pending = await waiting;
  events.push(pending === value ? "pending" : "BAD");
  console.log(events.join(","));
}
run();
