// @ts-nocheck
async function load(
  specifier:
    | "./fixtures/dynamic-param/one.ts"
    | "./fixtures/dynamic-param/two.ts",
) {
  const mod = await import(specifier);
  return `${mod.name}:${mod.value}`;
}

async function loadByName(name: "one" | "two") {
  const mod = await import(`./fixtures/dynamic-param/${name}.ts`);
  return `${mod.name}:${mod.value}`;
}

console.log("param one:", await load("./fixtures/dynamic-param/one.ts"));
console.log("param two:", await load("./fixtures/dynamic-param/two.ts"));
console.log("template one:", await loadByName("one"));
console.log("template two:", await loadByName("two"));
