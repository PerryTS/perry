function fromInput(input: any): Record<string, string> {
  if (Symbol.iterator in input) {
    return Object.fromEntries(input as Iterable<[string, string]>);
  }
  return Object.fromEntries(Object.entries(input));
}

const headers = new Headers({ "Content-Type": "text/plain", "X-Id": "7" });
console.log(JSON.stringify(fromInput(headers)));

const form = new FormData();
form.append("k", "v");
const objects: Record<string, any> = {
  Headers: headers,
  FormData: form,
  Request: new Request("http://example.test/"),
  Response: new Response("body"),
  Blob: new Blob(["x"]),
  File: new File(["x"], "x.txt"),
  responseHeaders: new Response("body", { headers: { a: "1" } }).headers,
};
for (const [label, value] of Object.entries(objects)) {
  console.log(
    label,
    Symbol.iterator in value,
    Symbol.asyncIterator in value,
    Symbol.toStringTag in value,
    value.constructor?.name,
  );
}
console.log(objects.Headers.constructor === Headers, objects.Response.constructor === Response);
console.log(JSON.stringify(fromInput(objects.responseHeaders)));

const ownHeaders: any = new Headers();
ownHeaders.constructor = 123;
console.log("own constructor", ownHeaders.constructor === 123);
ownHeaders.constructor = undefined;
console.log("undefined constructor", ownHeaders.constructor === undefined);
delete ownHeaders.constructor;

const originalHeaders = Headers;
(globalThis as any).Headers = class ReplacementHeaders {};
console.log("intrinsic constructor", ownHeaders.constructor === originalHeaders);
(globalThis as any).Headers = originalHeaders;
const prototype: any = originalHeaders.prototype;
prototype.constructor = 456;
console.log("prototype constructor", ownHeaders.constructor === 456);
prototype.constructor = undefined;
console.log("undefined prototype constructor", ownHeaders.constructor === undefined);
prototype.constructor = originalHeaders;

let constructorCalls = 0;
Object.defineProperty(ownHeaders, "constructor", {
  get() {
    constructorCalls++;
    return undefined;
  },
});
console.log("constructor getter", ownHeaders.constructor === undefined, constructorCalls);

const ownSymbol = Symbol("own");
ownHeaders[ownSymbol] = undefined;
console.log("undefined symbol present", ownSymbol in ownHeaders);
const accessorSymbol = Symbol("accessor");
let getterCalls = 0;
Object.defineProperty(ownHeaders, accessorSymbol, {
  get() {
    getterCalls++;
    return undefined;
  },
});
console.log("accessor present", accessorSymbol in ownHeaders, getterCalls);
console.log("accessor read", ownHeaders[accessorSymbol], getterCalls);

const handlePrototype: any = new Headers();
handlePrototype.constructor = 789;
const inheritedHeaders: any = new Headers();
Object.setPrototypeOf(inheritedHeaders, handlePrototype);
console.log("handle prototype constructor", inheritedHeaders.constructor);

const customHeaders: any = new Headers({ base: "old" });
let iteratorCalls = 0;
customHeaders[Symbol.iterator] = function () {
  iteratorCalls++;
  return [["custom", "new"]][Symbol.iterator]();
};
console.log("custom iterator", JSON.stringify(Object.fromEntries(customHeaders)), iteratorCalls);
customHeaders[Symbol.iterator] = undefined;
try {
  Object.fromEntries(customHeaders);
  console.log("undefined iterator rejected", false);
} catch (error) {
  console.log("undefined iterator rejected", error instanceof TypeError);
}
