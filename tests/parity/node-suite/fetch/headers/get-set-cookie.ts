const h = new Headers();

console.log("method type:", typeof h.getSetCookie);
console.log("empty:", JSON.stringify(h.getSetCookie()));

h.append("set-cookie", "a=1");
h.append("X-Test", "x");
h.append("SET-cookie", "b=2");
h.append("x-test", "y");

console.log("cookies:", JSON.stringify(h.getSetCookie()));
console.log("set-cookie get:", h.get("set-cookie"));
console.log("entries:", JSON.stringify(Array.from(h.entries())));
console.log("keys:", JSON.stringify(Array.from(h.keys())));
console.log("values:", JSON.stringify(Array.from(h.values())));

const seen: string[] = [];
h.forEach((value, key) => seen.push(key + "=" + value));
console.log("forEach:", JSON.stringify(seen));

h.set("set-cookie", "c=3");
console.log("set replaces:", JSON.stringify(h.getSetCookie()), h.get("set-cookie"));

h.delete("set-cookie");
console.log("delete clears:", JSON.stringify(h.getSetCookie()), h.has("set-cookie"));

const other = new Headers();
other.append("cookie", "not-set");
console.log("non set-cookie:", JSON.stringify(other.getSetCookie()));
