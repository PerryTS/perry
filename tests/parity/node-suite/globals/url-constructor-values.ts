import { URL as ModuleURL, URLSearchParams as ModuleURLSearchParams } from "node:url";

const g = globalThis as any;

for (const name of ["URL", "URLSearchParams"]) {
  const desc = Object.getOwnPropertyDescriptor(globalThis, name)!;
  console.log(
    name,
    "descriptor:",
    desc.writable,
    desc.enumerable,
    desc.configurable,
    typeof desc.value,
    desc.value.name,
    Object.keys(globalThis).includes(name),
  );
}

console.log("URL identity:", ModuleURL === URL, ModuleURL === g.URL);
console.log(
  "URLSearchParams identity:",
  ModuleURLSearchParams === URLSearchParams,
  ModuleURLSearchParams === g.URLSearchParams,
);

console.log(
  "global canParse:",
  g.URL.canParse("https://example.com/a"),
  typeof g.URL.canParse("https://example.com/a"),
  g.URL.canParse("not a url"),
);
console.log("global parse invalid:", g.URL.parse("not a url"));
console.log("global parse valid:", g.URL.parse("https://example.com/a?b=1")?.href);

const ReboundURL = globalThis.URL;
const ReboundURLSearchParams = globalThis.URLSearchParams;
console.log("rebound URL:", new ReboundURL("/p", "https://example.com/base").href);
console.log("rebound params:", new ReboundURLSearchParams("a=1").get("a"));

const { URL: DestructuredURL, URLSearchParams: DestructuredURLSearchParams } = globalThis as any;
console.log("destructured URL:", new DestructuredURL("https://example.com/d").href);
console.log("destructured params:", new DestructuredURLSearchParams("b=2").get("b"));

console.log("chained rebound params:", new ReboundURLSearchParams("c=3").get("c"));
console.log("chained destructured params:", new DestructuredURLSearchParams("d=4").get("d"));
