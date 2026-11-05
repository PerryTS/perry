import { collect } from "perry/gc";
import { imports, register } from "./generated/guest";
const http = imports["wasi:http/types@0.2.12"];
const streams = imports["wasi:io/streams@0.2.12"];
const poll = imports["wasi:io/poll@0.2.12"];
const errors = imports["wasi:io/error@0.2.12"];
let mode = 0;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
function success(result: { tag: string }, message: string): void { check(result.tag === "ok", message); }
function collectGuest(): void {
  console.error("HTTP-GUEST-GC-START");
  collect();
  console.error("HTTP-GUEST-GC-END");
}
function finishIncoming(body: Parameters<typeof http["[static]incoming-body.finish"]>[0]): void {
  const future = http["[static]incoming-body.finish"](body);
  const ready = http["[method]future-trailers.subscribe"](future.borrow());
  try {
    poll["[method]pollable.block"](ready.borrow());
    collectGuest();
    const result = http["[method]future-trailers.get"](future.borrow());
    check(result.tag === "some", "trailers readiness");
    if (result.tag === "some") {
      check(result.value.tag === "ok", "trailers single consumption");
      if (result.value.tag === "ok") {
        check(result.value.value.tag === "ok", "trailers transport");
        if (result.value.value.tag === "ok" && result.value.value.value.tag === "some") {
          result.value.value.value.value.dispose();
        }
      }
    }
  } finally { ready.dispose(); future.dispose(); }
}
function readBody(body: Parameters<typeof http["[static]incoming-body.finish"]>[0]): Uint8Array {
  const opened = http["[method]incoming-body.stream"](body.borrow());
  if (opened.tag !== "ok") { body.dispose(); throw new Error("incoming stream"); }
  const input = opened.value;
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const read = streams["[method]input-stream.blocking-read"](input.borrow(), 4096n);
      if (read.tag === "err") {
        if (read.value.tag === "closed") break;
        const error = read.value.value;
        const message = errors["[method]error.to-debug-string"](error.borrow());
        error.dispose();
        throw new Error(message);
      }
      chunks.push(read.value);
      length += read.value.length;
      collectGuest();
    }
  } finally { input.dispose(); }
  finishIncoming(body);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { output.set(chunk, offset); offset += chunk.length; }
  return output;
}
function outgoing(port: number, payload: Uint8Array): { tag: "ok", value: Uint8Array } | { tag: "err", value: string } {
  const headers = http["[constructor]fields"]();
  const request = http["[constructor]outgoing-request"](headers);
  success(http["[method]outgoing-request.set-method"](request.borrow(), { tag: "post" }), "request method");
  success(http["[method]outgoing-request.set-scheme"](request.borrow(), { tag: "some", value: { tag: "HTTP" } }), "request scheme");
  success(http["[method]outgoing-request.set-authority"](request.borrow(), { tag: "some", value: "127.0.0.1:" + port }), "request authority");
  success(http["[method]outgoing-request.set-path-with-query"](request.borrow(), { tag: "some", value: "/echo" }), "request path");
  const bodyResult = http["[method]outgoing-request.body"](request.borrow());
  if (bodyResult.tag !== "ok") { request.dispose(); return { tag: "err", value: "request body" }; }
  const body = bodyResult.value;
  const submitted = imports["wasi:http/outgoing-handler@0.2.12"].handle(request, { tag: "none" });
  if (submitted.tag !== "ok") { body.dispose(); return { tag: "err", value: submitted.value.tag }; }
  const future = submitted.value;
  const ready = http["[method]future-incoming-response.subscribe"](future.borrow());
  let finished = false;
  try {
    const writer = http["[method]outgoing-body.write"](body.borrow());
    if (writer.tag !== "ok") return { tag: "err", value: "request writer" };
    const stream = writer.value;
    try {
      for (let offset = 0; offset < payload.length; offset += 4096) {
        const result = streams["[method]output-stream.blocking-write-and-flush"](stream.borrow(), payload.slice(offset, offset + 4096));
        if (result.tag !== "ok") {
          if (result.value.tag === "last-operation-failed") result.value.value.dispose();
          return { tag: "err", value: "upload failed" };
        }
        collectGuest();
      }
    } finally { stream.dispose(); }
    const completed = http["[static]outgoing-body.finish"](body, { tag: "none" });
    finished = true;
    if (completed.tag !== "ok") return { tag: "err", value: completed.value.tag };
    poll["[method]pollable.block"](ready.borrow());
    collectGuest();
    const result = http["[method]future-incoming-response.get"](future.borrow());
    if (result.tag !== "some") return { tag: "err", value: "response readiness" };
    if (result.value.tag !== "ok") return { tag: "err", value: "response consumed" };
    if (result.value.value.tag !== "ok") return { tag: "err", value: result.value.value.value.tag };
    const response = result.value.value.value;
    try {
      check(http["[method]incoming-response.status"](response.borrow()) === 200, "outgoing response status");
      const received = http["[method]incoming-response.consume"](response.borrow());
      if (received.tag !== "ok") return { tag: "err", value: "response body" };
      return { tag: "ok", value: readBody(received.value) };
    } finally { response.dispose(); }
  } finally {
    if (!finished) body.dispose();
    ready.dispose();
    future.dispose();
  }
}
try { register({
  configure(value) { mode = value; },
  outgoing(port, payload) {
    try { return outgoing(port, payload); }
    catch (error) { console.error("HTTP-GUEST-OUTGOING-ERROR", error); throw error; }
  },
  "wasi:http/incoming-handler@0.2.12": {
    handle(request, responseOut) {
      collectGuest();
      if (mode === 1) { request.dispose(); responseOut.dispose(); return; }
      if (mode === 2) throw new Error("Perry HTTP guest trap");
      if (mode === 4) { while (true) { /* engine fuel bounds this loop */ } }
      if (mode === 3) {
        request.dispose();
        http["[static]response-outparam.set"](responseOut, { tag: "err", value: { tag: "internal-error", value: { tag: "some", value: "explicit" } } });
        return;
      }
      check(http["[method]incoming-request.method"](request.borrow()).tag === "post", "incoming method");
      const received = http["[method]incoming-request.consume"](request.borrow());
      if (received.tag !== "ok") throw new Error("incoming body");
      const body = received.value;
      const reader = http["[method]incoming-body.stream"](body.borrow());
      if (reader.tag !== "ok") throw new Error("incoming stream");
      const input = reader.value;
      const response = http["[constructor]outgoing-response"](http["[constructor]fields"]());
      success(http["[method]outgoing-response.set-status-code"](response.borrow(), 202), "response status");
      const responseBody = http["[method]outgoing-response.body"](response.borrow());
      if (responseBody.tag !== "ok") throw new Error("outgoing body");
      const outputBody = responseBody.value;
      const writer = http["[method]outgoing-body.write"](outputBody.borrow());
      if (writer.tag !== "ok") throw new Error("outgoing stream");
      const output = writer.value;
      http["[static]response-outparam.set"](responseOut, { tag: "ok", value: response });
      try {
        while (true) {
          const chunk = streams["[method]input-stream.blocking-read"](input.borrow(), 4096n);
          if (chunk.tag === "err") {
            if (chunk.value.tag === "closed") break;
            chunk.value.value.dispose();
            throw new Error("request body failure");
          }
          success(streams["[method]output-stream.blocking-write-and-flush"](output.borrow(), chunk.value), "response write");
          collectGuest();
        }
      } finally { input.dispose(); output.dispose(); }
      success(http["[static]outgoing-body.finish"](outputBody, { tag: "none" }), "response finish");
      finishIncoming(body);
      request.dispose();
    }
  }
}); } catch (error) { console.error("HTTP-GUEST-INIT-ERROR", error); throw error; }
