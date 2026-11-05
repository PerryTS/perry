// ArrayBuffer-family toString ignores its arguments; Buffer.toString uses them.
// S() produces an SSO string at runtime rather than a heap literal.
const S = (s: string): string => s.charAt(0) + s.slice(1);

const ab = new ArrayBuffer(1);
new Uint8Array(ab)[0] = 65;
const sab = new SharedArrayBuffer(1);
new Uint8Array(sab)[0] = 65;
const dv = new DataView(ab);
const objectWithThrowingToString = { toString() { throw new Error('must-not-coerce'); } };

for (const [name, value] of [['ArrayBuffer', ab], ['SharedArrayBuffer', sab], ['DataView', dv]] as const) {
  const anyValue = value as any;
  console.log(name, 'no-args', JSON.stringify(anyValue.toString()));
  console.log(name, 'runtime-short', JSON.stringify(anyValue.toString(S('hex'))));
  console.log(name, 'heap-literal', JSON.stringify(anyValue.toString('hex')));
  console.log(name, 'extra-range', JSON.stringify(anyValue.toString(S('hex'), 0, 1)));
  try { console.log(name, 'throwing-arg', JSON.stringify(anyValue.toString(objectWithThrowingToString))); }
  catch (error) { console.log(name, 'throwing-arg', 'THREW', (error as Error).message); }
}

const buffer = Buffer.from([65]);
console.log('Buffer runtime-short', JSON.stringify(buffer.toString(S('hex'))));
console.log('Buffer heap-literal', JSON.stringify(buffer.toString('hex')));
console.log('Buffer extra-range', JSON.stringify(buffer.toString('hex', 0, 1)));
