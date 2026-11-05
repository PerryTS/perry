// #11591: a `.ts` entry is a module under Node (ESM or CommonJS), so its
// top-level declarations are module-scoped and never become properties of
// the global object — in particular they must not shadow a builtin global.
function foo() { return 1; }
function URL(x?: string) { return { x }; }
var topVar = 42;
const G: any = globalThis;
console.log(typeof G.foo, G.URL === URL, typeof G.URL);
console.log("own foo", Object.prototype.hasOwnProperty.call(globalThis, "foo"));
console.log("own topVar", Object.prototype.hasOwnProperty.call(globalThis, "topVar"), G.topVar);
console.log("builtin URL intact", new G.URL("https://example.com/a").pathname);
// Module bindings themselves still work.
console.log(foo(), URL("u").x, topVar);
