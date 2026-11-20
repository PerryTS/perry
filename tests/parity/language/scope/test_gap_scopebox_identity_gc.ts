declare function gc(): void;
function afterCollection(value: any) {
  for (let i = 0; i < 4; i++) {
    const garbage = new Array(32);
    garbage[0] = value;
  }
  if (typeof gc === 'function') gc();
  return value;
}
function comparisons() {
  const fixed = { value: 'scopebox-' + process.argv.length };
  const other = { value: 'different-' + process.argv.length };
  let correct = 0;
  for (let i = 0; i < 20; i++) {
    if (fixed === afterCollection(fixed)) correct++;
    if (fixed !== afterCollection(other)) correct++;
    if (fixed !== afterCollection(Symbol.for('scopebox'))) correct++;
  }
  console.log(correct);
}
comparisons();
