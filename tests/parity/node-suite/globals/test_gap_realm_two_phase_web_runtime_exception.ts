class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
try {
  structuredClone(function uncloneable() {});
} catch (error) {
  console.log(error.name, error.code);
  console.log(error instanceof DOMException, error instanceof Error);
  console.log(Object.getPrototypeOf(error) === DOMException.prototype);
}
