// Runtime trip counts keep the dynamic read sites live. Compare with Node.
const N = Number(process.argv[2] ?? 200000);
const mode = process.argv[3] ?? 'wide';
function read(o: any, key: any): any { return o[key]; }
const objects: any[] = [];
const holder: any = { inherited: 7 };
const keys: string[] = [];
const large: any = {};
for (let i = 0; i < 64; i++) {
  const o: any = Object.create(holder);
  o['padding' + i] = i;
  o.value = i;
  objects.push(o);
  keys.push('key' + i);
  large['key' + i] = i;
}
let sum = 0;
for (let i = 0; i < N; i++) {
  const at = i % 64;
  if (mode === 'wide') sum += read(objects[at], 'value');
  else if (mode === 'keys') sum += read(large, keys[at]);
  else if (mode === 'inherited') sum += read(objects[at], 'inherited');
  else if (mode === 'numeric') sum += read(keys, at).length;
  else throw new Error('unknown mode');
}
console.log(sum);
