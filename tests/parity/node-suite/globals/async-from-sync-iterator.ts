async function runFullIteration(): Promise<void> {
  const log: string[] = [];
  let idx = 0;
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          log.push("next:" + idx);
          if (idx === 0) {
            idx++;
            return { value: Promise.resolve(10), done: false };
          }
          if (idx === 1) {
            idx++;
            return { value: 20, done: false };
          }
          return { value: 0, done: true };
        },
      };
    },
  };

  let sum = 0;
  for await (const value of iterable) {
    log.push("body:" + value);
    sum += value;
  }
  console.log("full:", log.join("|"), "sum=" + sum);
}

async function runEarlyBreak(): Promise<void> {
  const log: string[] = [];
  let idx = 0;
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          log.push("next:" + idx);
          if (idx === 0) {
            idx++;
            return { value: Promise.resolve("first"), done: false };
          }
          if (idx === 1) {
            idx++;
            return { value: Promise.resolve("second"), done: false };
          }
          return { value: undefined, done: true };
        },
        return(value?: unknown) {
          log.push("return:" + String(value));
          return { value: "closed", done: true };
        },
      };
    },
  };

  for await (const value of iterable) {
    log.push("body:" + value);
    break;
  }
  console.log("break:", log.join("|"));
}

async function runArrayFromAsync(): Promise<void> {
  const log: string[] = [];
  let idx = 0;
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          log.push("next:" + idx);
          if (idx === 0) {
            idx++;
            return { value: Promise.resolve("a"), done: false };
          }
          if (idx === 1) {
            idx++;
            return { value: "b", done: false };
          }
          return { value: undefined, done: true };
        },
      };
    },
  };

  const out = await Array.fromAsync(iterable);
  console.log("array:", out.join(","), log.join("|"));
}

async function runDestructuring(): Promise<void> {
  let idx = 0;
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          if (idx === 0) {
            idx++;
            return { value: Promise.resolve(["pair", 7]), done: false };
          }
          return { value: undefined, done: true };
        },
      };
    },
  };

  for await (const [label, value] of iterable) {
    console.log("destructure:", label + ":" + value);
  }
}

async function runRejectedValue(): Promise<void> {
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          return { value: Promise.reject("boom"), done: false };
        },
      };
    },
  };

  try {
    for await (const value of iterable) {
      console.log("rejected-value: body:" + value);
    }
  } catch (error) {
    console.log("rejected-value:", String(error));
  }
}

async function runBadIteratorResult(): Promise<void> {
  const iterable = {
    [Symbol.iterator]() {
      return {
        next() {
          return 1;
        },
      };
    },
  };

  try {
    await Array.fromAsync(iterable);
    console.log("bad-result: resolved");
  } catch (error) {
    console.log("bad-result:", (error as Error).constructor.name);
  }
}

await runFullIteration();
await runEarlyBreak();
await runArrayFromAsync();
await runDestructuring();
await runRejectedValue();
await runBadIteratorResult();
