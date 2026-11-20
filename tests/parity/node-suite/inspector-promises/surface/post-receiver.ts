import { Session } from "node:inspector/promises";

let returnedPromise = false;
try {
  const pending = Session.prototype.post.call({}, "Runtime.enable");
  returnedPromise = pending instanceof Promise;
  await pending;
  console.log("unexpected resolution");
} catch (error) {
  const cause = error as { name?: string; message?: string };
  console.log(
    "receiver:",
    returnedPromise,
    cause.name,
    cause.message?.includes("#connection"),
  );
}
