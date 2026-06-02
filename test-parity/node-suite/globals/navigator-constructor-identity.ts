const nav = (globalThis as any).navigator;
const Nav = (globalThis as any).Navigator;

console.log("navigator typeof:", typeof nav);
console.log("Navigator typeof:", typeof Nav);
console.log("Navigator name:", Nav?.name);
console.log("Navigator length:", Nav?.length);
console.log("constructor identity:", nav?.constructor === Nav);
console.log("instanceof Navigator:", nav instanceof Nav);
console.log("prototype constructor:", Object.getPrototypeOf(nav)?.constructor === Nav);
console.log("userAgent prefix:", typeof nav?.userAgent === "string" && nav.userAgent.startsWith("Node.js/"));
console.log("languages first:", nav?.languages?.[0]);
try {
  new Nav();
} catch (err: any) {
  console.log("new Navigator throws:", err.name, err.message);
}
