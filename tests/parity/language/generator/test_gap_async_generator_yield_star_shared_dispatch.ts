// Async-generator `yield*` throw routes must select the active delegation and
// resume through one shared state dispatcher. This checks both separate
// delegation sites and a delegation-protocol error caught by the outer
// generator. The transform has a separate structural growth test ensuring the
// shared dispatcher is not cloned once per route.

async function* inner(label: string) {
  try {
    yield label + ":first";
  } catch (error) {
    return label + ":caught:" + error;
  }
}

async function* outer() {
  const first = yield* inner("a");
  yield "after-a:" + first;
  const second = yield* inner("b");
  yield "after-b:" + second;
}

class BrokenDelegate {
  [Symbol.asyncIterator]() {
    return this;
  }

  next() {
    return Promise.resolve({ value: "broken:first", done: false });
  }

  throw() {
    throw new Error("delegate-fail");
  }
}

async function* catchesProtocolError() {
  try {
    yield* new BrokenDelegate();
  } catch (error: any) {
    yield "outer-caught:" + error.message;
  }
}

async function main() {
  const iterator = outer();
  console.log(JSON.stringify(await iterator.next()));
  console.log(JSON.stringify(await iterator.throw("boom")));
  console.log(JSON.stringify(await iterator.next()));
  console.log(JSON.stringify(await iterator.throw("bang")));
  console.log(JSON.stringify(await iterator.next()));

  const broken = catchesProtocolError();
  console.log(JSON.stringify(await broken.next()));
  console.log(JSON.stringify(await broken.throw("ignored")));
  console.log(JSON.stringify(await broken.next()));
}

main();
