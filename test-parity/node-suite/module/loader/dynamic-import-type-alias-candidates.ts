async function load(specifier: AliasSpecifier) {
  const mod = await import(specifier);
  return specifier + ":" + mod.value;
}

async function loadByName(name: AliasName) {
  const mod = await import(`./fixtures/dynamic-alias/${name}.ts`);
  return name + ":" + mod.value;
}

console.log("direct a:", await load("./fixtures/dynamic-alias/a.ts"));
console.log("direct b:", await load("./fixtures/dynamic-alias/b.ts"));
console.log("template a:", await loadByName("a"));
console.log("template b:", await loadByName("b"));

type AliasSpecifier =
  | "./fixtures/dynamic-alias/a.ts"
  | "./fixtures/dynamic-alias/b.ts";

type AliasName = "a" | "b";
