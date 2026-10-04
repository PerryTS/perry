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

// The third read uses GetV, unlike the second read's language in operator.
for (const entry of [
  ['null', null], ['undefined', undefined], ['object', { name: 'ThirdObject' }],
  ['number', 0], ['boolean', false], ['string', ''], ['symbol', Symbol('third')], ['bigint', 0n],
]) {
  let calls = 0;
  const value: any = {};
  Object.defineProperty(value, 'constructor', {
    get() { calls++; if (typeof gc === 'function') gc(); return calls < 3 ? { name: 'Second' } : entry[1]; },
  });
  callbackError('third-constructor-' + entry[0], value);
  console.log('third-constructor-' + entry[0] + '-calls', calls);
}
for (const entry of [
  ['number', Number.prototype, 0], ['boolean', Boolean.prototype, false],
  ['string', String.prototype, ''], ['symbol', Symbol.prototype, Symbol('third')],
  ['bigint', BigInt.prototype, 0n],
]) {
  const proto: any = entry[1];
  const original = Object.getOwnPropertyDescriptor(proto, 'name');
  let receiver = false;
  let nameCalls = 0;
  let calls = 0;
  try {
    Object.defineProperty(proto, 'name', {
      get() { 'use strict'; nameCalls++; receiver = this === entry[2]; if (typeof gc === 'function') gc(); return 'PrimitiveName'; },
      configurable: true,
    });
    const value: any = {};
    Object.defineProperty(value, 'constructor', {
      get() { calls++; return calls < 3 ? { name: 'Second' } : entry[2]; },
    });
    callbackError('third-primitive-name-' + entry[0], value);
    console.log('third-primitive-name-' + entry[0] + '-reads', calls, nameCalls, receiver);
  } finally {
    if (original) Object.defineProperty(proto, 'name', original);
    else delete proto.name;
  }
}

const remainingNatives: any[] = [
  ['arraybuffer', ArrayBuffer.prototype, new ArrayBuffer(1)],
  ['sharedarraybuffer', SharedArrayBuffer.prototype, new SharedArrayBuffer(1)],
  ['date', Date.prototype, new Date(0)],
];
for (const entry of remainingNatives) {
  const label = entry[0], proto = entry[1], value = entry[2];
  const original = Object.getOwnPropertyDescriptor(proto, 'constructor');
  try {
    Object.defineProperty(proto, 'constructor', { value: { name: 'PatchedNative' }, configurable: true });
    callbackError('native-' + label + '-data', value);
    for (const unit of [0xd800, 0xdc00]) {
      Object.defineProperty(proto, 'constructor', { value: { name: String.fromCharCode(unit) }, configurable: true });
      try { fs.exists('/received-diagnostic-unused', value); } catch (error: any) {
        const start = error.message.indexOf('an instance of ') + 15;
        console.log('native-' + label + '-unit', unit, error.name, error.code, error.message.charCodeAt(start));
      }
    }
    for (const mode of ['getter', 'throw', 'collect']) {
      let calls = 0;
      let receiver = true;
      Object.defineProperty(proto, 'constructor', {
        get() {
          calls++; receiver = receiver && this === value;
          if (mode === 'throw') throw new Error('constructor sentinel');
          if (mode === 'collect' && typeof gc === 'function') gc();
          return { name: 'PatchedNative' };
        }, configurable: true,
      });
      callbackError('native-' + label + '-' + mode, value);
      console.log('native-' + label + '-' + mode + '-reads', calls, receiver);
    }
  } finally { Object.defineProperty(proto, 'constructor', original!); }
  callbackError('native-' + label + '-restored', value);
}
const fallbackNatives: any[] = [
  ...remainingNatives,
  ['int16array', Int16Array.prototype, new Int16Array(1)],
  ['dataview', DataView.prototype, new DataView(new ArrayBuffer(1))],
  ['uint8array', Uint8Array.prototype, new Uint8Array(1)],
];
for (const entry of fallbackNatives) {
  const label = entry[0], proto = entry[1], value = entry[2];
  try {
    Object.defineProperty(value, 'constructor', { value: undefined, configurable: true });
    callbackError('fallback-' + label + '-own-undefined', value);
    delete value.constructor;
    Object.setPrototypeOf(value, null);
    callbackError('fallback-' + label + '-null', value);
    Object.setPrototypeOf(value, Object.create(null));
    callbackError('fallback-' + label + '-custom-missing', value);
  } finally { Object.setPrototypeOf(value, proto); }
  callbackError('fallback-' + label + '-restored', value);
}

for (const entry of fallbackNatives) {
  const value = entry[2], proto = entry[1];
  try {
    Object.setPrototypeOf(value, { constructor: undefined });
    callbackError('fallback-' + entry[0] + '-ordinary-undefined', value);
  } finally { Object.setPrototypeOf(value, proto); }
}
for (const value of [new Int16Array(0), new Uint8Array(0)]) {
  const proto = Object.getPrototypeOf(value);
  try {
    Object.defineProperty(value, 'constructor', { value: undefined, configurable: true });
    callbackError('fallback-empty-view-own-undefined', value);
    delete value.constructor;
    Object.setPrototypeOf(value, Object.create(null));
    callbackError('fallback-empty-view-custom-missing', value);
  } finally { Object.setPrototypeOf(value, proto); }
}
const int16CtorDescriptor = Object.getOwnPropertyDescriptor(Int16Array.prototype, 'constructor');
try {
  Object.defineProperty(Int16Array.prototype, 'constructor', { value: undefined, configurable: true });
  callbackError('fallback-intrinsic-undefined', new Int16Array(1));
  callbackError('fallback-empty-intrinsic-undefined', new Int16Array(0));
  delete (Int16Array.prototype as any).constructor;
  callbackError('fallback-intrinsic-deleted', new Int16Array(1));
} finally { Object.defineProperty(Int16Array.prototype, 'constructor', int16CtorDescriptor!); }
let proxyReads = 0;
const proxyReceived = new Proxy({}, {
  get(target, key, receiver) {
    if (key === 'constructor') { proxyReads++; return { name: 'ProxyReceived' }; }
    return Reflect.get(target, key, receiver);
  },
});
callbackError('direct-proxy-constructor', proxyReceived);
console.log('direct-proxy-constructor-reads', proxyReads);
proxyReads = 0;
const proxyNullish = new Proxy({}, {
  get(target, key, receiver) {
    if (key === 'constructor') { proxyReads++; return proxyReads === 3 ? null : { name: 'ProxyReceived' }; }
    return Reflect.get(target, key, receiver);
  },
});
callbackError('direct-proxy-third-null', proxyNullish);
console.log('direct-proxy-third-null-reads', proxyReads);

// Native prototype chains: fallback classification must inspect link
// authority (never a user-visible constructor Get, accessor execution or
// Proxy trap), and the native Buffer payload arm performs exactly one extra
// tolerated constructor Get. chainReads counts constructor Gets on the
// received value; chainDeepReads must stay 0: no accessor installed on a
// chain link ever executes.
let chainReads = 0;
let chainDeepReads = 0;
function chainValueGetter(secondThrow: boolean): () => undefined {
  return () => {
    chainReads++;
    if (secondThrow && chainReads === 2) throw new Error('constructor sentinel');
    return undefined;
  };
}
function chainLinkGetter(): () => undefined {
  return () => {
    chainDeepReads++;
    return undefined;
  };
}
const chainNatives: any[] = [
  ['arraybuffer', ArrayBuffer.prototype, new ArrayBuffer(1)],
  ['int16array', Int16Array.prototype, new Int16Array(1)],
  ['date', Date.prototype, new Date(0)],
  ['dataview', DataView.prototype, new DataView(new ArrayBuffer(1))],
  ['buffer', Buffer.prototype, Buffer.alloc(1)],
];
const chainShapes: any[] = [
  ['native', (proto: any) => Object.create(proto)],
  ['deep-native', (proto: any) => Object.create(Object.create(proto))],
  ['deep-null', () => {
    const nullBorn: any = {};
    Object.setPrototypeOf(nullBorn, null);
    return Object.create(nullBorn);
  }],
  ['ordinary-missing', () => Object.create(Object.prototype)],
  ['ordinary-undefined', () => Object.create(Object.prototype)],
  ['explicit-null', () => null],
];
for (const entry of chainNatives) {
  const label = entry[0], proto = entry[1], value = entry[2];
  for (const shape of chainShapes) {
    for (const mode of ['data', 'getter']) {
      chainReads = 0;
      chainDeepReads = 0;
      try {
        const replacement = shape[1](proto);
        if (replacement !== null) {
          if (mode === 'getter') {
            Object.defineProperty(replacement, 'constructor',
              { get: chainLinkGetter(), configurable: true });
          } else if (shape[0] !== 'ordinary-missing') {
            Object.defineProperty(replacement, 'constructor',
              { value: undefined, configurable: true });
          }
        }
        Object.defineProperty(value, 'constructor',
          { get: chainValueGetter(false), configurable: true });
        Object.setPrototypeOf(value, replacement);
        callbackError('chain-' + label + '-' + shape[0] + '-' + mode, value);
        if (mode === 'getter') {
          console.log('chain-' + label + '-' + shape[0] + '-reads',
            chainReads, chainDeepReads);
        }
      } finally {
        delete value.constructor;
        Object.setPrototypeOf(value, proto);
      }
    }
    if (label === 'buffer' && (shape[0] === 'native' || shape[0] === 'deep-native')) {
      chainReads = 0;
      chainDeepReads = 0;
      try {
        const replacement = shape[1](proto);
        Object.defineProperty(value, 'constructor',
          { get: chainValueGetter(true), configurable: true });
        Object.setPrototypeOf(value, replacement);
        callbackError('chain-' + label + '-' + shape[0] + '-second-throw', value);
        console.log('chain-' + label + '-' + shape[0] + '-second-throw-reads',
          chainReads, chainDeepReads);
      } finally {
        delete value.constructor;
        Object.setPrototypeOf(value, proto);
      }
    }
  }
  callbackError('chain-' + label + '-restored', value);
}
