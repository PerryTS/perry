// @ts-nocheck
async function loadFromLet(flag: boolean) {
  let specifier = flag
    ? "./fixtures/dynamic-local-a.ts"
    : "./fixtures/dynamic-local-b.ts";
  const mod = await import(specifier);
  return mod.value;
}

async function loadFromTemplate(flag: boolean) {
  let name = flag ? "a" : "b";
  let specifier = `./fixtures/dynamic-local-${name}.ts`;
  const mod = await import(specifier);
  return mod.templateValue;
}

console.log("let true:", await loadFromLet(true));
console.log("let false:", await loadFromLet(false));
console.log("template true:", await loadFromTemplate(true));
console.log("template false:", await loadFromTemplate(false));
