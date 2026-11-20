import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();

const result = await first.run("first", () =>
  second.run("second", async () => {
    const exited = first.exit(async () => {
      console.log(
        "isolated exit sync stores:",
        String(first.getStore()),
        second.getStore(),
      );
      await Promise.resolve();
      console.log(
        "isolated exit continuation stores:",
        String(first.getStore()),
        second.getStore(),
      );
      return "exit-result";
    });

    console.log(
      "isolated exit immediate restore:",
      first.getStore(),
      second.getStore(),
    );
    return await exited;
  }),
);

console.log("isolated exit result:", result);
console.log(
  "isolated exit outside:",
  String(first.getStore()),
  String(second.getStore()),
);
first.disable();
second.disable();
