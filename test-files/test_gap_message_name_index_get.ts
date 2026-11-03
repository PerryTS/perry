// `x.message[k]` / `x.name[k]` lower through a string fast path because
// `.message` / `.name` are guessed to be strings (Error fields). The value
// decides: a Proxy, an object, an array or a Map-like holder under those
// names reads its property. (A Solid store's `store.session.message[id]`
// read undefined, so opencode's session view showed no messages.)
const key = ["s", "1"].join("");
const store: any = { session: { message: new Proxy({ s1: ["m1", "m2"] } as Record<string, any>, {}) } };
console.log("proxy", JSON.stringify(store.session.message[key]), JSON.stringify(store.session.message["s1"]));
const trapped: any = { name: new Proxy({}, { get: (_t, k) => "trap:" + String(k) }) };
console.log("trap", trapped.name[key], trapped.name[0]);
const obj: any = { message: { s1: 1, 2: "two" }, name: ["a", "b", "c"] };
console.log("object", obj.message[key], obj.message[2], obj.name[1], obj.name.length);
const err = new Error("boom");
console.log("error", err.message[0], err.message[3], err.name[0], err.message.length);
const strs: any = { message: "hello", name: "world" };
console.log("string", strs.message[1], strs.name[key], strs.name[4]);
function read(holder: any, k: any) { return holder.message[k]; }
console.log("fn", JSON.stringify(read(store.session, key)), read(strs, 0), read({ message: [7, 8] }, 1));
try { read({}, 0); } catch (e: any) { console.log("undefined receiver", e instanceof TypeError); }
