// @ts-nocheck
async function loadFromBranch(flag: boolean) {
  let specifier;
  if (flag) {
    specifier = "./fixtures/dynamic-local-a.ts";
  } else {
    specifier = "./fixtures/dynamic-local-b.ts";
  }
  const mod = await import(specifier);
  return mod.value;
}

async function loadFromReassignedTernary(flag: boolean) {
  let specifier;
  specifier = flag
    ? "./fixtures/dynamic-local-a.ts"
    : "./fixtures/dynamic-local-b.ts";
  const mod = await import(specifier);
  return mod.templateValue;
}

async function loadFromBranchTemplate(flag: boolean) {
  let specifier;
  if (flag) {
    specifier = `./fixtures/dynamic-local-a.ts`;
  } else {
    specifier = `./fixtures/dynamic-local-b.ts`;
  }
  const mod = await import(specifier);
  return `${mod.value}:${mod.templateValue}`;
}

console.log("branch true:", await loadFromBranch(true));
console.log("branch false:", await loadFromBranch(false));
console.log("reassign true:", await loadFromReassignedTernary(true));
console.log("reassign false:", await loadFromReassignedTernary(false));
console.log("template true:", await loadFromBranchTemplate(true));
console.log("template false:", await loadFromBranchTemplate(false));
