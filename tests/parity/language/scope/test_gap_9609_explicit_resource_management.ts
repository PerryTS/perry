// #9609: enabling SWC's JavaScript explicit-resource-management syntax must
// not disturb Perry's existing using/await-using lowering. This fixture is
// compiled by Perry and compared byte-for-byte with the pinned Node version.

const events: string[] = [];

function syncResource(name: string, disposeError?: string) {
  return {
    [Symbol.dispose]() {
      events.push("dispose:" + name);
      if (disposeError) {
        throw new Error(disposeError);
      }
    },
  };
}

function reverseOrder(): void {
  using first = syncResource("first");
  using second = syncResource("second");
  events.push("body:reverse");
}

function returnCompletion(): string {
  using resource = syncResource("return");
  events.push("body:return");
  return "returned";
}

function throwCompletion(): void {
  try {
    using resource = syncResource("throw");
    events.push("body:throw");
    throw new Error("body-failure");
  } catch (error: any) {
    events.push("caught:" + error.message);
  }
}

function suppressedCompletion(): void {
  try {
    using first = syncResource("suppressed-first", "dispose-first");
    using second = syncResource("suppressed-second", "dispose-second");
    events.push("body:suppressed");
    throw new Error("body-failure");
  } catch (error: any) {
    events.push(
      [
        "suppressed",
        error instanceof SuppressedError,
        error.error.message,
        error.suppressed instanceof SuppressedError,
        error.suppressed.error.message,
        error.suppressed.suppressed.message,
      ].join(":"),
    );
  }
}

async function asyncOrder(): Promise<void> {
  await using first = {
    async [Symbol.asyncDispose]() {
      await Promise.resolve();
      events.push("async-dispose:first");
    },
  };
  await using second = {
    async [Symbol.asyncDispose]() {
      await Promise.resolve();
      events.push("async-dispose:second");
    },
  };
  events.push("body:async");
}

async function main(): Promise<void> {
  reverseOrder();
  events.push("result:" + returnCompletion());
  throwCompletion();
  suppressedCompletion();
  await asyncOrder();
  console.log(events.join("\n"));
}

main();
