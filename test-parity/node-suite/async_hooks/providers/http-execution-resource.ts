import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { get, createServer } from "node:http";

const resources = new Map<number, object>();
const hook = createHook({
  init(asyncId, _type, _triggerAsyncId, resource) {
    resources.set(asyncId, resource);
  },
}).enable();
const server = createServer((_request, response) => response.end("ok"));
let request: ReturnType<typeof get> | undefined;
let completed = false;

try {
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(resolve, 1_000);
    const finish = () => {
      completed = true;
      clearTimeout(timeout);
      resolve();
    };
    const fail = (error: Error) => {
      clearTimeout(timeout);
      reject(error);
    };
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      console.log(
        "http listen resource mapped:",
        executionAsyncResource() === resources.get(executionAsyncId()),
      );
      const address = server.address();
      if (!address || typeof address === "string") {
        fail(new Error("missing address"));
        return;
      }
      request = get({ host: "127.0.0.1", port: address.port }, (response) => {
        console.log(
          "http response resource mapped:",
          executionAsyncResource() === resources.get(executionAsyncId()),
        );
        response.once("error", fail);
        response.resume();
        response.once("end", finish);
      });
      request.once("error", fail);
    });
  });
} finally {
  request?.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
  hook.disable();
}
console.log("http execution resource completed:", completed);
