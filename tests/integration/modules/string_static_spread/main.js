// Independent regression: static String calls must retain spread boundaries.
let failures = 0;
function check(name, run, expected) {
  try {
    const actual = run();
    if (actual !== expected) throw new Error('unexpected result: ' + actual);
    console.log('PASS ' + name);
  } catch (error) {
    failures++;
    console.log('FAIL ' + name + ': ' + error.message);
  }
}
const points = [65, 66, 0x1f600];
check('codepoint spread', () => String.fromCodePoint(...points), 'AB😀');
check('codepoint empty', () => String.fromCodePoint(...[]), '');
check('codepoint mixed', () => String.fromCodePoint(64, ...points, 67, ...[68]), '@AB😀CD');
check('codepoint computed', () => String['fromCodePoint'](...points), 'AB😀');
const fromCodePoint = String.fromCodePoint;
check('codepoint detached', () => fromCodePoint(...points), 'AB😀');
check('codepoint scalars', () => String.fromCodePoint(65, 66, 0x1f600), 'AB😀');
check('codepoint typed array', () => String.fromCodePoint(...new Uint32Array([65, 0x1f600])), 'A😀');
check('codepoint set', () => String.fromCodePoint(...new Set([65, 66])), 'AB');
check('charcode spread', () => String.fromCharCode(...[65, 66]), 'AB');
check('charcode empty', () => String.fromCharCode(...[]), '');
check('charcode mixed', () => String.fromCharCode(64, ...[65, 66], 67, ...[68]), '@ABCD');
check('charcode computed', () => String['fromCharCode'](64, ...[65, 66]), '@AB');
check('charcode surrogate pair', () => String.fromCharCode(65, ...[0xd83d, 0xde00]), 'A😀');
const template = { raw: ['a', 'b', 'c'] };
check('raw spread substitutions', () => String.raw(template, ...[1, 2]), 'a1b2c');
check('raw whole spread', () => String.raw(...[template, 1, 2]), 'a1b2c');
check('raw mixed spread', () => String.raw(template, 1, ...[2]), 'a1b2c');
check('codepoint evaluation order', () => {
  let order = '';
  function part(mark, value) { order += mark; return value; }
  const result = String.fromCodePoint(part('a', 65), ...part('b', [66]), part('c', 67));
  return order + ':' + result;
}, 'abc:ABC');
check('codepoint rejects invalid spread elements', () => {
  let caught = 0;
  for (const value of [-1, 1.5, 0x110000, NaN, Infinity]) {
    try { String.fromCodePoint(...[65, value]); }
    catch (error) { if (error.name === 'RangeError') caught++; }
  }
  return caught;
}, 5);
check('ordinary array argument is not spread', () => {
  try { String.fromCodePoint([65, 66]); }
  catch (error) { return error.name; }
  return 'did not throw';
}, 'RangeError');
check('spread operands survive collection', () => {
  function collect(value) { if (typeof globalThis.gc === 'function') globalThis.gc(); return value; }
  let result = '';
  for (let index = 0; index < 12; index++) {
    result = String.fromCodePoint(...collect([65]), collect(66), ...collect([0x1f600]));
    if (result !== 'AB😀') throw new Error('spread roots lost');
  }
  return result;
}, 'AB😀');
if (failures !== 0) throw new Error('String static spread failures: ' + failures);
console.log('PASS String static spread regression');
