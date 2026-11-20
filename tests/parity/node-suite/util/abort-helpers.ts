import util, {
  aborted,
  transferableAbortController,
  transferableAbortSignal,
} from "node:util";

async function rejected(label, fn) {
  try {
    const promise = fn();
    console.log(label, "returns promise:", promise instanceof Promise);
    await promise;
    console.log(label, "resolved");
  } catch (error) {
    console.log(
      label,
      "rejected",
      error.name,
      error.code,
      String(error.message).split("\n")[0],
    );
  }
}

console.log(
  "helpers:",
  typeof util.aborted,
  typeof aborted,
  util.aborted === aborted,
  typeof transferableAbortController,
  typeof transferableAbortSignal,
);

const controller = new AbortController();
const promise = aborted(controller.signal, {});
let resolved = false;
promise.then((event) => {
  resolved = true;
  console.log("resolved event:", event.type, event.target === controller.signal);
});

console.log("promise:", promise instanceof Promise);
await Promise.resolve();
console.log("before abort resolved:", resolved);
controller.abort("done");
await promise;
console.log(
  "after abort:",
  resolved,
  controller.signal.aborted,
  controller.signal.reason,
);

const preAborted = new AbortController();
preAborted.abort("pre");
const preEvent = await aborted(preAborted.signal, {});
console.log(
  "pre aborted:",
  preEvent === undefined,
  preAborted.signal.aborted,
  preAborted.signal.reason,
);

const transferable = transferableAbortController();
console.log(
  "controller initial:",
  typeof transferable.abort,
  transferable.signal.aborted,
  String(transferable.signal.reason),
);
transferable.abort("x");
console.log(
  "controller after:",
  transferable.signal.aborted,
  transferable.signal.reason,
);

const source = new AbortController();
const signal = transferableAbortSignal(source.signal);
console.log("signal identity:", typeof signal, signal === source.signal, signal.aborted);
source.abort("later");
console.log("signal after source abort:", signal.aborted, signal.reason);

await rejected("invalid aborted signal", () => aborted(undefined, {}));
await rejected("invalid aborted signal null", () => aborted(null, {}));
await rejected("invalid aborted signal object", () => aborted({}, {}));
await rejected("invalid aborted signal number", () => aborted(1, {}));
await rejected("invalid aborted resource", () =>
  aborted(new AbortController().signal, undefined),
);

try {
  transferableAbortSignal({});
} catch (error) {
  console.log(
    "invalid transferable signal",
    error.name,
    error.code,
    String(error.message).split("\n")[0],
  );
}
