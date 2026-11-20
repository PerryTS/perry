function show(label: string, value: unknown) {
  console.log(`${label}:`, typeof value === "string" ? JSON.stringify(value) : String(value));
}

show("FormData typeof", typeof FormData);
show("FormData name", FormData.name);
show("FormData length", FormData.length);

for (const method of [
  "append",
  "delete",
  "entries",
  "forEach",
  "get",
  "getAll",
  "has",
  "keys",
  "set",
  "values",
]) {
  const fn = (FormData.prototype as any)[method];
  console.log(`proto ${method}:`, typeof fn, fn.name, fn.length);
}

const fd = new FormData();
show("empty missing", fd.get("missing") === null);
show("empty has a", fd.has("a"));

fd.append("a", "1");
fd.append("a", "2");
fd.append("b", "bee");
fd.append("num", 42 as any);

show("get a", fd.get("a"));
console.log("getAll a:", JSON.stringify(fd.getAll("a")));
show("get num", fd.get("num"));
console.log("entries initial:", JSON.stringify(Array.from(fd.entries())));
console.log("keys initial:", JSON.stringify(Array.from(fd.keys())));
console.log("values initial:", JSON.stringify(Array.from(fd.values())));

fd.set("a", "replaced");
fd.delete("b");
fd.delete("missing");

show("has a", fd.has("a"));
show("has b", fd.has("b"));
show("get a after set", fd.get("a"));
console.log("getAll a after set:", JSON.stringify(fd.getAll("a")));
console.log("entries final:", JSON.stringify(Array.from(fd.entries())));
console.log("keys final:", JSON.stringify(Array.from(fd.keys())));
console.log("values final:", JSON.stringify(Array.from(fd.values())));

const seen: string[] = [];
fd.forEach((value, key, owner) => {
  seen.push(`${key}=${value}:${owner === fd ? "self" : "other"}`);
});
console.log("forEach:", JSON.stringify(seen));

function useAny(form: any) {
  console.log("any methods:", typeof form.append, typeof form.get, typeof form.entries);
  form.append("c", "see");
  console.log("any get c:", JSON.stringify(form.get("c")));
}

useAny(fd);
