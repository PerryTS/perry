// Calling a method that neither the Buffer API nor %TypedArray%.prototype
// implements must throw a TypeError like Node — not silently return undefined.
//
// Pre-fix, `dispatch_buffer_method`'s catch-all returned undefined for any
// unknown name. That silence turned the readFileSync Node-parity migration
// (no encoding → Buffer, not string) into invisible data corruption for
// callers still treating the result as a string: `content.charCodeAt(i)`
// yielded undefined in every comparison and the program kept running as if
// the calls had worked (real case: an editor's NUL-byte binary-file scan
// misclassified every text file it opened).

const buf = Buffer.from('hello', 'utf8');

// The implemented Buffer API surface keeps working.
console.log('toString:', buf.toString('utf8'));
console.log('indexOf:', buf.indexOf('l'));

// Inherited %TypedArray%.prototype methods keep working via delegation.
console.log('every:', buf.every((b) => b > 0));

// A String.prototype method reached through a Buffer receiver throws.
try {
  (buf as any).charCodeAt(0);
  console.log('charCodeAt: no throw');
} catch (e) {
  console.log('charCodeAt throws TypeError:', e instanceof TypeError);
  const msg = (e as Error).message;
  console.log('mentions method:', msg.includes('charCodeAt'));
  console.log('mentions not-a-function:', msg.includes('is not a function'));
}

// A method that exists nowhere throws too.
try {
  (buf as any).definitelyNotAMethod(1, 2);
  console.log('bogus: no throw');
} catch (e) {
  console.log('bogus throws TypeError:', e instanceof TypeError);
  console.log(
    'mentions not-a-function:',
    (e as Error).message.includes('is not a function')
  );
}

// The throw is catchable and execution continues normally afterwards.
console.log('still running:', buf.readUInt8(0));
