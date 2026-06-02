import urlDefault, * as url from "node:url";
import { URLPattern } from "node:url";

function show(label: string, value: unknown) {
  console.log(`${label}:`, value);
}

function showError(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(
      `${label}: ok`,
      value === null ? "null" : typeof value,
      value === null ? "null" : String(value),
    );
  } catch (error: any) {
    console.log(`${label}: err`, error.name, error.code || "no-code", error.message);
  }
}

show("global typeof", typeof globalThis.URLPattern);
show("module typeof", typeof URLPattern);
show("identity global", URLPattern === globalThis.URLPattern);
show("identity default", (urlDefault as any).URLPattern === URLPattern);
show("identity namespace", (url as any).URLPattern === URLPattern);
show("keys has URLPattern", Object.keys(urlDefault as any).includes("URLPattern"));
show("name length", `${(URLPattern as any).name} ${(URLPattern as any).length}`);
show(
  "proto",
  Object.getOwnPropertyNames((URLPattern as any).prototype).sort().join(","),
);
show(
  "descriptor",
  Object.getOwnPropertyDescriptor(urlDefault as any, "URLPattern")?.enumerable,
);

const objectPattern = new URLPattern({ pathname: "/users/:id" });
show(
  "object props",
  ["protocol", "username", "password", "hostname", "port", "pathname", "search", "hash"]
    .map((key) => `${key}=${(objectPattern as any)[key]}`)
    .join("|"),
);
show(
  "object hasRegExpGroups",
  `${typeof objectPattern.hasRegExpGroups} ${objectPattern.hasRegExpGroups === false}`,
);
show("object test full", objectPattern.test("https://example.com/users/42"));
const objectExec = objectPattern.exec("https://example.com/users/42")!;
show("object exec id", objectExec.pathname.input + "|" + objectExec.pathname.groups.id);
show("object exec inputs", objectExec.inputs.join("|"));
const objectMiss = objectPattern.exec("https://example.com/nope");
show("object exec miss", objectMiss === null ? "null" : typeof objectMiss);

const stringPattern = new URLPattern("https://example.com/books/:slug");
show("string test", stringPattern.test("https://example.com/books/node"));
show(
  "string exec slug",
  stringPattern.exec("https://example.com/books/node")!.pathname.groups.slug,
);

const basePattern = new URLPattern("/items/:id", "https://base.example");
show("base test", basePattern.test("https://base.example/items/7"));
show("base props", `${basePattern.protocol}|${basePattern.hostname}|${basePattern.pathname}`);
const baseExec = basePattern.exec("/items/8", "https://base.example")!;
show("base exec", baseExec.inputs.join("|") + "|" + baseExec.pathname.groups.id);

showError("call without new", () => (URLPattern as any)({ pathname: "/x" }));
showError("bad input test", () => objectPattern.test({}));
showError("bad input exec", () => objectPattern.exec({}));
