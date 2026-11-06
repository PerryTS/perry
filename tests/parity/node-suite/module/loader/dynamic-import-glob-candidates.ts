// @ts-nocheck
async function load(name: string) {
  const mod = await import(`./fixtures/dynamic-glob/${name}.ts`);
  return `${mod.name}:${mod.value}`;
}

console.log("glob alpha:", await load("alpha"));
console.log("glob beta:", await load("beta"));
