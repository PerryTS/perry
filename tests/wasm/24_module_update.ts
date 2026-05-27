// Module-level let update expressions must read from and write back to the
// WASM global that backs the top-level binding.
let ticks = 0;

function bumpPostfix(): number {
  const before = ticks++;
  console.log(before);
  console.log(ticks);
  return ticks;
}

function bumpPrefix(): number {
  const after = ++ticks;
  console.log(after);
  console.log(ticks);
  return ticks;
}

console.log(ticks);
bumpPostfix();
console.log(ticks);
bumpPrefix();
console.log(ticks);
ticks--;
console.log(ticks);
--ticks;
console.log(ticks);
