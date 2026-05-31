import * as url from "node:url";
import {
  URL as ImportedURL,
  URLSearchParams as ImportedURLSearchParams,
} from "node:url";
import {
  TextDecoder as UtilTextDecoder,
  TextEncoder as UtilTextEncoder,
} from "node:util";

const g = globalThis as any;

console.log("URL identity:", URL === g.URL, url.URL === g.URL, ImportedURL === g.URL);
const globalUrl = new globalThis.URL("/path?q=1", "https://example.com/base");
console.log("global URL:", globalUrl.href, globalUrl.searchParams.get("q"));
const namespaceUrl = new url.URL("https://example.com/a?b=2");
console.log("namespace URL:", namespaceUrl.href, namespaceUrl.searchParams.get("b"));
const reboundUrl = new ImportedURL("https://example.com/rebound?ok=1");
console.log("imported URL:", reboundUrl.href, reboundUrl.searchParams.get("ok"));
const dynamicUrl = new g.URL("https://example.com/dynamic?ok=2");
console.log("dynamic URL:", dynamicUrl.href);

console.log(
  "URLSearchParams identity:",
  URLSearchParams === g.URLSearchParams,
  url.URLSearchParams === g.URLSearchParams,
  ImportedURLSearchParams === g.URLSearchParams,
);
const globalParams = new globalThis.URLSearchParams("a=1&a=2");
console.log("global params:", globalParams.getAll("a").join("|"));
const namespaceParams = new url.URLSearchParams({ b: "2", a: "1" });
namespaceParams.sort();
console.log("namespace params:", namespaceParams.toString());

console.log(
  "encoding identity:",
  TextEncoder === g.TextEncoder,
  TextDecoder === g.TextDecoder,
  UtilTextEncoder === g.TextEncoder,
  UtilTextDecoder === g.TextDecoder,
);
console.log("inline encode:", Array.from(new globalThis.TextEncoder().encode("ok")).join(","));
const encoder = new UtilTextEncoder();
const encoded = encoder.encode("cafe");
console.log("imported encode:", Array.from(encoded).join(","));
console.log("dynamic encoder typeof:", typeof new g.TextEncoder());
const decoder = new globalThis.TextDecoder("utf-8", { fatal: true });
console.log("global decoder:", decoder.encoding, decoder.fatal, decoder.decode(encoded));
