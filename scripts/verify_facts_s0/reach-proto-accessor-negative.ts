// A getter on String.prototype read on a string. Without the escape of an
// accessor installed on a built-in prototype (`noreachprotoaccessors`) no
// lookup the solve walks reaches it, and its body is called inactive.
function read(o: any, k: string): any { return o[k]; }
Object.defineProperty(String.prototype, "shout", {
  get(this: any) { return String(this).toUpperCase() + "!"; }, configurable: true,
});
console.log(read("abc", "shout"));
