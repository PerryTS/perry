import tls from "node:tls";

const controller = new AbortController();
controller.abort();
let secure = false;
let abortError = false;
let reported = false;
const socket = tls.connect({
  host: "127.0.0.1",
  port: 1,
  signal: controller.signal,
});
socket.on("secureConnect", () => {
  secure = true;
  socket.destroy();
});
socket.on(
  "error",
  (err: any) => {
    abortError ||= err.name === "AbortError" && err.code === "ABORT_ERR";
  },
);
socket.on("close", () => {
  if (reported) return;
  reported = true;
  console.log("result:", secure, abortError);
});
