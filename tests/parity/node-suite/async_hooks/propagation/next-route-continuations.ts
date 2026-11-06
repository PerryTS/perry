import { AsyncLocalStorage } from "node:async_hooks";

// Focused lower-level mirror of #8034's production Next App Route sequence.
// These are the four independent stores Next wires around routeModule.handle.
const actionStorage = new AsyncLocalStorage<string>();
const requestStorage = new AsyncLocalStorage<string>();
const workStorage = new AsyncLocalStorage<string>();
const workUnitStorage = new AsyncLocalStorage<string>();

function stores(): string {
  return [
    actionStorage.getStore(),
    requestStorage.getStore(),
    workStorage.getStore(),
    workUnitStorage.getStore(),
  ]
    .map((value) => value ?? "none")
    .join("/");
}

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const firstGate = deferred();
const secondStarted = deferred();

async function handle(id: string, iterations: number): Promise<string> {
  return actionStorage.run(`action-${id}`, () =>
    requestStorage.run(`request-${id}`, () =>
      workStorage.run(`work-${id}`, () =>
        workUnitStorage.run(`unit-${id}`, async () => {
          const observations = [stores()];

          if (id === "first") {
            secondStarted.resolve();
            await firstGate.promise;
          } else if (id === "second") {
            await secondStarted.promise;
            firstGate.resolve();
          }

          const { checksum } = await import("./fixtures/next-route-lazy.js");
          observations.push(stores());

          await Promise.resolve().then(() => {
            observations.push(stores());
          });
          await new Promise<void>((resolve) => setTimeout(resolve, 1));
          observations.push(stores());

          try {
            requestStorage.run(`request-${id}-throw`, () => {
              observations.push(stores());
              throw new Error(`throw-${id}`);
            });
          } catch (error) {
            observations.push(`${(error as Error).message}:${stores()}`);
          }

          try {
            await requestStorage.run(`request-${id}-nested`, async () => {
              await Promise.resolve();
              observations.push(stores());
              throw new Error(`reject-${id}`);
            });
          } catch (error) {
            observations.push(`${(error as Error).message}:${stores()}`);
          }

          const exited = await workUnitStorage.exit(async () => {
            await new Promise<void>((resolve) => queueMicrotask(resolve));
            return stores();
          });
          observations.push(`exit=${exited}`);
          observations.push(stores());

          const stream = new ReadableStream<string>({
            start(controller) {
              queueMicrotask(() => {
                controller.enqueue(stores());
                controller.close();
              });
            },
          });
          const streamed = await stream.getReader().read();
          observations.push(`stream=${streamed.value}`);

          return `${id}:${checksum(iterations)}:${observations.join("|")}`;
        }),
      ),
    ),
  );
}

const results = await Promise.all([handle("first", 7), handle("second", 11)]);
for (const result of results) console.log(result);
console.log(await handle("after-rejection", 13));
console.log("next route outside:", stores());
