// Buffer.concat over a list array that grew past the storage it was born
// with: `[chunk]` returned from a function is allocated at exact capacity,
// and the first push moves its elements. Buffer.concat read the old slots and
// rejected `list[1]` (effect's NodeStream pull: `Arr.of(item)` then `push`;
// every opencode git command that printed more than one chunk failed).
const of = <A>(a: A): [A] => [a];
const one = (a: any) => [a];
const b = (s: string) => Buffer.from(s);
const t1 = of(b("ab")); t1.push(b("cd")); t1.push(b("ef"));
console.log(Buffer.concat(t1).toString(), Buffer.concat(t1, 3).toString());
const t2: Uint8Array[] = one(new Uint8Array([104, 105])); t2.push(b("!"));
console.log(Buffer.concat(t2).toString());
const chunks = of(b("x"));
for (let i = 0; i < 100; i++) chunks.push(b(String(i % 10)));
console.log(Buffer.concat(chunks).length, Buffer.concat(chunks).toString().slice(0, 12));
const grown: Buffer[] = [];
for (let i = 0; i < 5; i++) grown.push(b("z"));
console.log(Buffer.concat(grown).toString());
