// Read the native super method before evaluating a spread, then call it
// with the subclass instance as receiver (#11910 / superemit).
const { EventEmitter } = require('node:events');

class Forward extends EventEmitter {
  emit(ev, ...args) { return super.emit(ev, ...args); }
  on(...args) { return super.on(...args); }
  once(...args) { return super.once(...args); }
  plain() { return super.emit('plain'); }
  send(args) { return super.emit('data', ...args); }
}
const emitter = new Forward();
emitter.on('data', (a, b) => console.log('data', a, b));
emitter.once('data', (a, b) => console.log('once', a, b));
emitter.on('plain', () => console.log('plain listener'));
console.log('on receiver', emitter.on('unused', () => {}) === emitter);
console.log('emit', emitter.emit('data', 1, 2));
console.log('emit again', emitter.emit('data', 3, 4));
console.log('missing', emitter.emit('missing'));
console.log('plain', emitter.plain());

// A getter runs before the spread; changing the property while iterating
// must not replace the function value that the call already read.
const original = EventEmitter.prototype.emit;
const order = [];
Object.defineProperty(EventEmitter.prototype, 'emit', {
  configurable: true,
  get() {
    order.push(this === emitter ? 'get receiver' : 'get wrong receiver');
    return original;
  },
});
const iterable = {
  [Symbol.iterator]() {
    order.push('iterate');
    Object.defineProperty(EventEmitter.prototype, 'emit', {
      configurable: true, writable: true,
      value() { console.log('replacement called'); return false; },
    });
    return [5, 6][Symbol.iterator]();
  },
};
console.log('ordered', emitter.send(iterable));
console.log('order', order.join(','));
Object.defineProperty(EventEmitter.prototype, 'emit', {
  configurable: true, enumerable: true, writable: true, value: original,
});

const { Readable } = require('node:stream');
class Source extends Readable {
  push(...args) { return super.push(...args); }
}
const source = new Source({ read() {} });
console.log('push', source.push('abc'));
console.log('read', source.read().toString());
console.log('end push', source.push(null));
