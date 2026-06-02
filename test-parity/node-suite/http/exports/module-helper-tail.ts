// #3712: node:http module-level helper/export tail — `maxHeaderSize`,
// `globalAgent`, and the header validation / parser-proxy setter helpers.
import * as http from "node:http";
import {
  validateHeaderName,
  validateHeaderValue,
  setMaxIdleHTTPParsers,
  setGlobalProxyFromEnv,
  globalAgent,
  maxHeaderSize,
} from "node:http";

console.log("maxHeaderSize:", maxHeaderSize, typeof maxHeaderSize);
console.log("namespace maxHeaderSize:", http.maxHeaderSize === maxHeaderSize);
console.log("globalAgent type:", typeof globalAgent);
console.log("globalAgent.protocol:", globalAgent.protocol);
console.log("globalAgent.defaultPort:", globalAgent.defaultPort);
console.log("globalAgent.keepAlive:", globalAgent.keepAlive);
console.log("validateHeaderName:", typeof validateHeaderName);
console.log("validateHeaderValue:", typeof validateHeaderValue);
console.log("setMaxIdleHTTPParsers:", typeof setMaxIdleHTTPParsers);
console.log("setGlobalProxyFromEnv:", typeof setGlobalProxyFromEnv);

function check(label: string, fn: () => unknown) {
  try {
    const r = fn();
    console.log(`${label}: ok ${String(r)}`);
  } catch (e: any) {
    console.log(`${label}: ${e.name} [${e.code}] ${e.message}`);
  }
}

check("valid name", () => validateHeaderName("X-Foo"));
check("empty name", () => validateHeaderName(""));
check("space name", () => validateHeaderName("X Foo"));
check("valid value", () => validateHeaderValue("X-Foo", "bar"));
check("undefined value", () => validateHeaderValue("X-Foo", undefined as any));
check("newline value", () => validateHeaderValue("X-Foo", "bad\nvalue"));
check("setMaxIdleHTTPParsers", () => setMaxIdleHTTPParsers(4));
