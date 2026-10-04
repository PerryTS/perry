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

// Audit sibling intrinsic branches with the same inherited-metadata root cause.
declare function gc(): void;
const inheritedViews: any[] = [
  ['dataview', DataView.prototype, new DataView(new ArrayBuffer(1))],
  ['buffer', Buffer.prototype, Buffer.alloc(1)],
  ['uint8array', Uint8Array.prototype, new Uint8Array(1)],
];
for (const entry of inheritedViews) {
  const label = entry[0];
  const prototype = entry[1];
  const view = entry[2];
  const original = Object.getOwnPropertyDescriptor(prototype, 'constructor');
  try {
    Object.defineProperty(prototype, 'constructor', {
      value: { name: 'PatchedView' }, writable: true, configurable: true,
    });
    callbackError('inherited-' + label + '-data', view);
    for (const unit of [0xd800, 0xdc00]) {
      Object.defineProperty(prototype, 'constructor', {
        value: { name: String.fromCharCode(unit) }, configurable: true,
      });
      try {
        fs.exists('/received-diagnostic-unused', view);
      } catch (error: any) {
        const start = error.message.indexOf('an instance of ') + 15;
        console.log('inherited-' + label + '-surrogate', unit, error.name, error.code,
          error.message.charCodeAt(start), error.message.length - start);
      }
    }
    let calls = 0;
    Object.defineProperty(prototype, 'constructor', {
      get() { calls++; return { name: 'PatchedView' }; }, configurable: true,
    });
    callbackError('inherited-' + label + '-getter', view);
    console.log('inherited-' + label + '-getter-calls', calls);
    calls = 0;
    Object.defineProperty(prototype, 'constructor', {
      get() { calls++; throw new Error('constructor sentinel'); }, configurable: true,
    });
    callbackError('inherited-' + label + '-throw', view);
    console.log('inherited-' + label + '-throw-calls', calls);
    calls = 0;
    Object.defineProperty(prototype, 'constructor', {
      get() {
        calls++;
        if (typeof gc === 'function') gc();
        return { name: 'PatchedView' };
      }, configurable: true,
    });
    callbackError('inherited-' + label + '-collect', view);
    console.log('inherited-' + label + '-collect-calls', calls);
  } finally {
    Object.defineProperty(prototype, 'constructor', original!);
  }
  callbackError('inherited-' + label + '-restored', view);
}

// Constructor metadata evaluates the language in operator, not boxed presence.
const primitiveConstructors: any[] = [
  ['number', 1], ['string', 'ctor'], ['boolean', true], ['symbol', Symbol('n')],
  ['null', null], ['undefined', undefined], ['false', false],
];
for (const entry of primitiveConstructors) {
  callbackError('primitive-constructor-' + entry[0], { constructor: entry[1] });
}
const primitiveViews: any[] = [
  ['buffer', Buffer.prototype, Buffer.alloc(1)],
  ['dataview', DataView.prototype, new DataView(new ArrayBuffer(1))],
  ['int16array', Int16Array.prototype, new Int16Array(1)],
];
for (const entry of primitiveViews) {
  const label = entry[0];
  const prototype = entry[1];
  const view = entry[2];
  const original = Object.getOwnPropertyDescriptor(prototype, 'constructor');
  try {
    for (const own of [true, false]) {
      const target = own ? view : prototype;
      let calls = 0;
      Object.defineProperty(target, 'constructor', {
        get() { calls++; if (typeof gc === 'function') gc(); return 1; },
        configurable: true,
      });
      callbackError('primitive-' + label + (own ? '-own' : '-prototype'), view);
      console.log('primitive-' + label + (own ? '-own-calls' : '-prototype-calls'), calls);
      if (own) delete view.constructor;
    }
  } finally {
    Object.defineProperty(prototype, 'constructor', original!);
  }
  callbackError('primitive-' + label + '-restored', view);
}
// Truthiness applies to the first read only; the second RHS is checked as-is.
for (const entry of primitiveConstructors) {
  let calls = 0;
  const value: any = {};
  Object.defineProperty(value, 'constructor', {
    get() { calls++; return calls === 1 ? 1 : entry[1]; }, configurable: true,
  });
  callbackError('primitive-second-' + entry[0], value);
  console.log('primitive-second-' + entry[0] + '-calls', calls);
}
