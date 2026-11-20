function observe(name: string, value: any) {
  const held: any[] = [value];
  const object: any = { value };
  if (typeof globalThis.gc === "function") globalThis.gc();
  console.log(name, typeof held[0], Number.isNaN(held[0]),
    Array.isArray(object.value), !!object.value);
}
const headers = new Headers({ test: "yes" });
observe("headers", headers);
console.log("header", headers.get("test"), headers.has("test"));
console.log("header prototype", Object.getPrototypeOf(headers) === Headers.prototype);
console.log("header brand", headers instanceof Headers);
const response = new Response("ok", { status: 201 });
observe("response", response);
console.log("response", response.status, await response.text());
const blob = new Blob(["blob"]);
observe("blob", blob);
console.log("blob", blob.size, await blob.text());
const proxy = new Proxy({ x: 3 }, { get(target, key) { return target[key]; } });
observe("proxy object", proxy);
console.log("proxy field", proxy.x, JSON.stringify(proxy));
const arrayProxy = new Proxy([1, 2], {});
observe("proxy array", arrayProxy);
console.log("proxy array", arrayProxy.length, arrayProxy.join(","));
const functionProxy = new Proxy((n: number) => n + 1, {});
observe("proxy function", functionProxy);
console.log("proxy call", functionProxy(3));
const readable = new ReadableStream({ start(controller) { controller.enqueue("chunk"); controller.close(); } });
observe("readable", readable);
const reader = readable.getReader();
observe("reader", reader);
console.log("stream", (await reader.read()).value, (await reader.read()).done);
const writable = new WritableStream({ write(value) { console.log("write", value); } });
observe("writable", writable);
const writer = writable.getWriter();
observe("writer", writer);
await writer.write("chunk");
await writer.close();
const timer = setTimeout(() => {}, 10000);
observe("timer", timer);
clearTimeout(timer);
const map = new Map<any, string>();
map.set(headers, "headers"); map.set(proxy, "proxy"); map.set(readable, "stream");
if (typeof globalThis.gc === "function") globalThis.gc();
console.log("keys", map.get(headers), map.get(proxy), map.get(readable));
for (const number of [1, 0x40000, 0xf0001, 0x100000]) {
  console.log("number", typeof number, Number.isNaN(number), Array.isArray(number));
}
