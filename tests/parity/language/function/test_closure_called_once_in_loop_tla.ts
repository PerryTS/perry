// Top-level await: module init is async.
let s = 0;
for (let i = 0; i < 3; i++) {
  const p = { x: i };
  const f = () => p.x * 2;
  s += f();
  await Promise.resolve();
}
console.log("tla", s);
export {};
