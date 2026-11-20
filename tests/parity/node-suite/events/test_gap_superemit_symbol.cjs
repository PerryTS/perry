// A minipass-shaped native-base subclass: symbol-keyed helpers alongside
// an overridden emit that forwards the rest arguments to super.emit.
const { EventEmitter } = require('node:events');
const deliver = Symbol('deliver');
class Mini extends EventEmitter {
  [deliver](d) { return super.emit('data', d); }
  emit(ev, ...args) {
    if (ev === 'data') return this[deliver](args[0]);
    return super.emit(ev, ...args);
  }
}
const mini = new Mini();
console.log('inherited', typeof mini.on, typeof mini.once);
mini.on('data', d => console.log('data', d));
mini.once('end', (a, b) => console.log('end', a, b));
console.log('symbol', mini.emit('data', 7));
console.log('spread', mini.emit('end', 8, 9));
console.log('missing', mini.emit('end', 10, 11));
console.log('prototype', Object.getPrototypeOf(Mini.prototype) === EventEmitter.prototype);

// Merely capturing a computed key must preserve the native parent, even
// without an emit override; each factory evaluation owns its prototype.
function make(key) {
  return class extends EventEmitter {
    [key]() { return key; }
  };
}
const first = make(deliver);
const second = make(deliver);
const a = new first();
const b = new second();
console.log('factory on', typeof a.on, typeof b.on);
console.log('factory helper', a[deliver]() === deliver, b[deliver]() === deliver);
console.log('factory prototypes', first.prototype !== second.prototype,
  Object.getPrototypeOf(first.prototype) === EventEmitter.prototype,
  Object.getPrototypeOf(second.prototype) === EventEmitter.prototype);
