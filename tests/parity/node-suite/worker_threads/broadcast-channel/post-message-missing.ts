import { BroadcastChannel } from "node:worker_threads";

const channel = new BroadcastChannel("missing-message");
try {
  (channel.postMessage as any)();
  console.log("post: ok");
} catch (error: any) {
  console.log("post:", error?.name, error?.code ?? "");
}
channel.close();
