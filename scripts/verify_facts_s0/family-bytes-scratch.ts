// family_reach: a comparator sort of more than 64 elements uses two private
// Uint32 scratch cells of the runtime. They are no JavaScript values: the
// fact says the typed-array family is never created or observed in this
// program, and the birth witness must not report them.
const xs: number[] = [];
for (let i = 0; i < 100; i++) xs.push((i * 7919) % 101);
xs.sort((x, y) => x - y);
console.log(xs[0], xs[99], xs.length);
