// Instances of the lazily born web families keep node's identities and chains.
const G: any = globalThis as any;
const out: string[] = [];
const eq = (label: string, v: any) => out.push(label + ": " + String(v));
eq("headers", Object.getPrototypeOf(new G.Headers()) === G.Headers.prototype);
eq("blob", Object.getPrototypeOf(new G.Blob(["a"])) === G.Blob.prototype);
eq("file is blob", new G.File(["a"], "f.txt") instanceof G.Blob);
eq("request", new G.Request("http://example.test/") instanceof G.Request);
eq("formdata", new G.FormData().constructor === G.FormData);
eq("textencoder", new G.TextEncoder().encode("\u00e9").length);
eq("textdecoder", new G.TextDecoder().decode(new G.TextEncoder().encode("hi")));
eq("customevent chain", Object.getPrototypeOf(G.CustomEvent.prototype) === G.Event.prototype && Object.getPrototypeOf(G.CustomEvent) === G.Event);
eq("messageevent chain", Object.getPrototypeOf(G.MessageEvent.prototype) === G.Event.prototype && Object.getPrototypeOf(G.MessageEvent) === G.Event);
eq("broadcast chain", Object.getPrototypeOf(G.BroadcastChannel.prototype) === G.EventTarget.prototype && Object.getPrototypeOf(G.BroadcastChannel) === G.EventTarget);
const ce = new G.CustomEvent("x", { detail: 5 });
eq("customevent", ce instanceof G.Event && ce.detail === 5 && ce.type === "x");
const me = new G.MessageEvent("message", { data: 7 });
eq("messageevent", me instanceof G.Event && me.data === 7);
const mc = new G.MessageChannel();
eq("messagechannel", mc.port1 instanceof G.MessagePort);
mc.port1.close(); mc.port2.close();
const bc = new G.BroadcastChannel("lp2");
eq("broadcast", bc instanceof G.EventTarget && bc.name === "lp2");
bc.close();
const ds = new G.DisposableStack();
eq("disposablestack", ds.disposed === false);
eq("suppressederror", new G.SuppressedError(1, 2, "m") instanceof Error);
// web storage and crypto: accessors on globalThis (node 26)
for (const k of ["sessionStorage", "crypto"]) {
  const d: any = Object.getOwnPropertyDescriptor(G, k)!;
  eq(k + " descriptor", [typeof d.get, typeof d.set, d.enumerable, d.configurable, "value" in d].join(","));
}
const ld: any = Object.getOwnPropertyDescriptor(G, "localStorage")!;
eq("localStorage descriptor", [typeof ld.get, typeof ld.set, ld.enumerable, ld.configurable, "value" in ld].join(","));
eq("sessionStorage same", G.sessionStorage === G.sessionStorage);
eq("sessionStorage proto", Object.getPrototypeOf(G.sessionStorage) === G.Storage.prototype);
G.sessionStorage.setItem("b", "2");
eq("sessionStorage item", G.sessionStorage.getItem("b") + "," + G.sessionStorage.length);
eq("crypto instanceof Crypto", G.crypto instanceof G.Crypto);
console.log(out.join("\n"));
