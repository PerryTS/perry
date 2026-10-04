import fs from 'node:fs';
import { Buffer } from 'node:buffer';

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
  ['Buffer', Buffer.alloc(1)], ['Uint8Array', new Uint8Array(1)],
  ['Int16Array', new Int16Array(1)], ['DataView', new DataView(new ArrayBuffer(1))],
  ['ArrayBuffer', new ArrayBuffer(1)], ['Date', new Date(0)],
  ['Array', []], ['Object', {}], ['bigint', 123456789012345678901234567890n],
  ['custom-name', { constructor: { name: 'Custom' } }],
  ['empty-name', { constructor: { name: '' } }],
  ['undefined-name', { constructor: { name: undefined } }],
  ['null-prototype', Object.create(null)],
];
for (const entry of cases) callbackError(entry[0], entry[1]);
function namedReceived(): void {}
try {
  fs.statSync(namedReceived as any);
} catch (error: any) {
  console.log('named-function', error.name, error.code, error.message);
}
const renamedBuffer: any = Buffer.alloc(1);
renamedBuffer.constructor = { name: 'RenamedBuffer' };
callbackError('renamed-buffer', renamedBuffer);
class NamedView extends Uint8Array {}
callbackError('view-subclass', new NamedView(1));
callbackError('denormal', 5e-324);

// UTF-8 console output replaces lone surrogates, so inspect message code units.
try {
  fs.exists('/received-diagnostic-unused', 'a'.repeat(24) + '😀abcd' as any);
} catch (error: any) {
  const start = error.message.indexOf("type string ('") + 14;
  console.log('utf16-boundary-code-units', error.name, error.code,
    error.message.charCodeAt(start + 24), error.message.charCodeAt(start + 25));
}

callbackError('constructor-symbol-name', { constructor: { name: Symbol('n') } });
Object.defineProperty(namedReceived, 'name', { value: Symbol('n') });
try {
  fs.statSync(namedReceived as any);
} catch (error: any) {
  console.log('function-symbol-name', error.name, error.code, error.message);
}
callbackError('symbol-value', Symbol('n'));
callbackError('number-name', { constructor: { name: 42 } });

for (const unit of [0xd800, 0xdc00]) {
  const name = String.fromCharCode(unit);
  try {
    fs.exists('/received-diagnostic-unused', { constructor: { name } } as any);
  } catch (error: any) {
    const start = error.message.indexOf('an instance of ') + 15;
    console.log('constructor-surrogate-name', unit, error.name, error.code,
      error.message.charCodeAt(start), error.message.length - start);
  }
  Object.defineProperty(namedReceived, 'name', { value: name });
  try {
    fs.statSync(namedReceived as any);
  } catch (error: any) {
    const start = error.message.indexOf('Received function ') + 18;
    console.log('function-surrogate-name', unit, error.name, error.code,
      error.message.charCodeAt(start), error.message.length - start);
  }
}

// Intrinsic brands must not hide inherited constructor data or accessors.
const originalInt16Constructor = Object.getOwnPropertyDescriptor(Int16Array.prototype, 'constructor');
const patchedView: any = new Int16Array(1);
try {
  Object.defineProperty(Int16Array.prototype, 'constructor', {
    value: { name: 'PatchedView' }, writable: true, configurable: true,
  });
  callbackError('inherited-view-data', patchedView);
  let calls = 0;
  Object.defineProperty(Int16Array.prototype, 'constructor', {
    get() { calls++; return { name: 'PatchedView' }; }, configurable: true,
  });
  callbackError('inherited-view-getter', patchedView);
  console.log('inherited-view-getter-calls', calls);
  calls = 0;
  Object.defineProperty(Int16Array.prototype, 'constructor', {
    get() { calls++; throw new Error('constructor sentinel'); }, configurable: true,
  });
  callbackError('inherited-view-throw', patchedView);
  console.log('inherited-view-throw-calls', calls);
} finally {
  Object.defineProperty(Int16Array.prototype, 'constructor', originalInt16Constructor!);
}
callbackError('inherited-view-restored', patchedView);
