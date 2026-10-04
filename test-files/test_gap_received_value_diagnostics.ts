import fs from 'node:fs';

function callbackError(label: string, value: any): void {
  try {
    fs.exists('/received-diagnostic-unused', value);
    console.log(label, 'NO ERROR');
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

const cases: any[] = [
  ['undefined', undefined], ['null', null], ['false', false], ['true', true],
  ['NaN', NaN], ['negative-zero', -0], ['positive-exponent', 1e21],
  ['negative-exponent', 1e-7], ['large-integer', 1e20],
  ['string27', 'a'.repeat(27)], ['string28', 'a'.repeat(28)],
  ['string29', 'a'.repeat(29)], ['mixed-quotes', 'a\'"\\\n'],
  ['quoted-surrogate', "'" + 'a'.repeat(23) + '😀abcd'],
  ['bigint', 123456789012345678901234567890n],
  ['symbol-value', Symbol('n')],
];
for (const entry of cases) callbackError(entry[0], entry[1]);
callbackError('denormal', 5e-324);

// UTF-8 console output replaces lone surrogates, so inspect message code units.
try {
  fs.exists('/received-diagnostic-unused', 'a'.repeat(24) + '😀abcd' as any);
} catch (error: any) {
  const start = error.message.indexOf("type string ('") + 14;
  console.log('utf16-boundary-code-units', error.name, error.code,
    error.message.charCodeAt(start + 24), error.message.charCodeAt(start + 25));
}
