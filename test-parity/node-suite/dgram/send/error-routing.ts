import * as dgram from "node:dgram";

const lookupError = Object.assign(new Error("lookup failed"), {
  code: "ELOOKUP",
});
function createSocket() {
  return dgram.createSocket({
    type: "udp4",
    lookup(hostname, _family, callback) {
      queueMicrotask(() => {
        if (hostname === "127.0.0.1") callback(null, hostname, 4);
        else callback(lookupError);
      });
    },
  });
}

const target = dgram.createSocket("udp4");
const withCallback = createSocket();
try {
  await Promise.all([
    new Promise<void>((resolve) => target.bind(0, "127.0.0.1", resolve)),
    new Promise<void>((resolve) => withCallback.bind(0, "127.0.0.1", resolve)),
  ]);
  const port = target.address().port;

  let callbackErrorEvents = 0;
  const onCallbackError = () => callbackErrorEvents++;
  withCallback.on("error", onCallbackError);
  try {
    const callbackCode = await new Promise<string>((resolve) => {
      withCallback.send("x", port, "127.0.0.2", (error) => {
        resolve(error?.code ?? "none");
      });
    });
    console.log("callback route:", callbackCode, callbackErrorEvents);
  } finally {
    withCallback.removeListener("error", onCallbackError);
  }
} finally {
  await Promise.all([
    new Promise<void>((resolve) => withCallback.close(resolve)),
    new Promise<void>((resolve) => target.close(resolve)),
  ]);
}
