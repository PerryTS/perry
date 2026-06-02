// process.sourceMapsEnabled is a boolean; setSourceMapsEnabled toggles it.
console.log("is boolean:", typeof process.sourceMapsEnabled === "boolean");
console.log("setter is function:", typeof process.setSourceMapsEnabled === "function");
process.setSourceMapsEnabled(false);
console.log("initial false:", process.sourceMapsEnabled === false);
process.setSourceMapsEnabled(true);
console.log("after true:", process.sourceMapsEnabled === true);
process.setSourceMapsEnabled(false);
console.log("after false:", process.sourceMapsEnabled === false);
try {
  process.setSourceMapsEnabled(1 as any);
  console.log("invalid:", "NO_THROW");
} catch (err: any) {
  console.log("invalid:", err.name, err.code, err.message.split("\n")[0]);
}
