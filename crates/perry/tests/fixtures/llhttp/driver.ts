// #9611 llhttp differential driver.
//
// Drives undici's real llhttp build the way undici drives it: a windowed
// Uint8Array over the engine's linear memory is filled with the socket chunk,
// `llhttp_execute` runs, and the parser calls back into JS. Every callback is
// read BOTH ways — straight out of wasm linear memory, and through undici's
// own trick of mapping the wasm pointer back into the source chunk — and the
// two must agree, which is the views-observability contract the zero-copy
// binding must not break.
import { readFileSync } from "node:fs";

const out: string[] = [];
function log(s: string): void {
  out.push(s);
}

let llhttp: any = null;
let parser = 0;
let bufPtr = 0;
let bufSize = 0;
let chunk: Uint8Array = new Uint8Array(0);

function fromWasm(at: number, len: number): string {
  const win = new Uint8Array(llhttp.memory.buffer, at, len);
  let s = "";
  for (let i = 0; i < len; i++) s += String.fromCharCode(win[i]);
  return s;
}

function fromChunk(at: number, len: number): string {
  const off = at - bufPtr;
  let s = "";
  for (let i = 0; i < len; i++) s += String.fromCharCode(chunk[off + i]);
  return s;
}

// Order-sensitive digest, so a single wrong byte anywhere in a long span
// changes the line. Kept in exact double range: 1e9 * 31 + 255 is far below
// 2**53, so node and perry must agree bit for bit.
function digest(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 1000000007;
  return h;
}

function describe(s: string): string {
  if (s.length <= 64) return JSON.stringify(s);
  return "len=" + s.length +
    " head=" + JSON.stringify(s.slice(0, 16)) +
    " tail=" + JSON.stringify(s.slice(s.length - 16)) +
    " digest=" + digest(s);
}

function span(name: string, at: number, len: number): void {
  const viaMemory = fromWasm(at, len);
  const viaChunk = fromChunk(at, len);
  if (viaMemory === viaChunk) {
    log(name + " " + describe(viaMemory));
  } else {
    log(name + " DIVERGED memory=" + describe(viaMemory) + " chunk=" + describe(viaChunk));
  }
}

const imports = {
  env: {
    wasm_on_message_begin: (p: number): number => {
      log("message_begin parser_matches=" + (p === parser));
      return 0;
    },
    wasm_on_url: (p: number, at: number, len: number): number => {
      span("url", at, len);
      return 0;
    },
    wasm_on_status: (p: number, at: number, len: number): number => {
      span("status", at, len);
      return 0;
    },
    wasm_on_header_field: (p: number, at: number, len: number): number => {
      span("header_field", at, len);
      return 0;
    },
    wasm_on_header_value: (p: number, at: number, len: number): number => {
      span("header_value", at, len);
      return 0;
    },
    wasm_on_headers_complete: (p: number, status: number, upgrade: number, keepAlive: number): number => {
      log("headers_complete status=" + status + " upgrade=" + upgrade + " keepalive=" + keepAlive);
      return 0;
    },
    wasm_on_body: (p: number, at: number, len: number): number => {
      span("body", at, len);
      return 0;
    },
    wasm_on_message_complete: (p: number): number => {
      log("message_complete");
      return 0;
    },
  },
};

const bytes = new Uint8Array(readFileSync(process.argv[2]));
const mod = new WebAssembly.Module(bytes);
const inst = new WebAssembly.Instance(mod, imports);
llhttp = inst.exports;
llhttp._initialize();
log("initial_pages=" + llhttp.memory.buffer.byteLength / 65536);

const TYPE_RESPONSE = 2;

function toBytes(s: string): Uint8Array {
  const b = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) b[i] = s.charCodeAt(i) & 0xff;
  return b;
}

// undici's `execute`, byte for byte: grow the scratch buffer in 4 KiB
// granules, fill a WINDOWED view over linear memory, run the parser, then read
// the error position back out.
function execute(data: Uint8Array): void {
  chunk = data;
  if (data.length > bufSize) {
    if (bufPtr) llhttp.free(bufPtr);
    bufSize = Math.ceil(data.length / 4096) * 4096;
    bufPtr = llhttp.malloc(bufSize);
  }
  new Uint8Array(llhttp.memory.buffer, bufPtr, bufSize).set(data);
  const ret = llhttp.llhttp_execute(parser, bufPtr, data.length);
  const errPos = llhttp.llhttp_get_error_pos(parser) - bufPtr;
  log("execute len=" + data.length + " ret=" + ret + " errpos=" + errPos);
}

function runCase(name: string, wire: string, chunkSize: number): void {
  log("=== " + name + " chunk=" + chunkSize);
  parser = llhttp.llhttp_alloc(TYPE_RESPONSE);
  const all = toBytes(wire);
  if (chunkSize <= 0) {
    execute(all);
  } else {
    for (let i = 0; i < all.length; i += chunkSize) {
      execute(all.subarray(i, Math.min(i + chunkSize, all.length)));
    }
  }
  log("finish=" + llhttp.llhttp_finish(parser));
  log("keep_alive=" + llhttp.llhttp_should_keep_alive(parser));
  log("status_code=" + llhttp.llhttp_get_status_code(parser));
  log("http=" + llhttp.llhttp_get_http_major(parser) + "." + llhttp.llhttp_get_http_minor(parser));
  log("upgrade=" + llhttp.llhttp_get_upgrade(parser));
  log("errno=" + llhttp.llhttp_get_errno(parser));
  llhttp.llhttp_free(parser);
  parser = 0;
}

const CRLF = "\r\n";
const SIMPLE =
  "HTTP/1.1 200 OK" + CRLF +
  "Content-Type: text/plain" + CRLF +
  "Content-Length: 13" + CRLF +
  CRLF +
  "hello, world!";

const CHUNKED =
  "HTTP/1.1 200 OK" + CRLF +
  "Transfer-Encoding: chunked" + CRLF +
  "X-Trace: abc" + CRLF +
  CRLF +
  "5" + CRLF + "hello" + CRLF +
  "7" + CRLF + ", world" + CRLF +
  "0" + CRLF +
  "X-Trailer: done" + CRLF +
  CRLF;

const PIPELINED =
  "HTTP/1.1 204 No Content" + CRLF + CRLF +
  "HTTP/1.1 200 OK" + CRLF +
  "Content-Length: 2" + CRLF + CRLF + "ok";

const CONTINUE =
  "HTTP/1.1 100 Continue" + CRLF + CRLF +
  "HTTP/1.1 200 OK" + CRLF +
  "Content-Length: 4" + CRLF + CRLF + "done";

const MANY_HEADERS = (() => {
  let s = "HTTP/1.1 200 OK" + CRLF;
  for (let i = 0; i < 64; i++) s += "X-Header-" + i + ": value-" + i + CRLF;
  s += "Content-Length: 3" + CRLF + CRLF + "abc";
  return s;
})();

// Large enough that llhttp's malloc must grow the linear memory past its
// initial 2 pages, so the grow path runs with the real module.
const BIG_BODY = (() => {
  const size = 300 * 1024;
  let body = "";
  const line = "0123456789abcdef";
  while (body.length < size) body += line;
  body = body.slice(0, size);
  return "HTTP/1.1 200 OK" + CRLF + "Content-Length: " + size + CRLF + CRLF + body;
})();

for (const chunkSize of [0, 1, 4096]) {
  runCase("simple", SIMPLE, chunkSize);
  runCase("chunked", CHUNKED, chunkSize);
  runCase("pipelined", PIPELINED, chunkSize);
  runCase("continue", CONTINUE, chunkSize);
}
runCase("many_headers", MANY_HEADERS, 0);
runCase("many_headers_split", MANY_HEADERS, 13);
runCase("big_body_grows_memory", BIG_BODY, 0);
log("final_pages=" + llhttp.memory.buffer.byteLength / 65536);

console.log(out.join("\n"));
