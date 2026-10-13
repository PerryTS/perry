import { Readable } from 'node:stream';
console.log('constructor fixture', true);
let receiver;
Object.defineProperty(Readable.prototype, '_readableState', {
  configurable: true,
  set(state) {
    receiver = this;
    Object.defineProperty(this, '_readableState', { value: state, writable: true, enumerable: true, configurable: true });
  }
});
const options = {
  get read() {
    console.log('options read', Object.keys(receiver).join(','), Object.hasOwn(receiver, '_read'), Object.hasOwn(receiver, '_maxListeners'));
    return function read() {};
  },
  get destroy() {
    console.log('options destroy', Object.hasOwn(receiver, '_read'), Object.hasOwn(receiver, '_destroy'));
    return undefined;
  }
};
const r = new Readable(options);
console.log('after', Object.keys(r).join(','));
delete Readable.prototype._readableState;
const plain = new Readable();
console.log('optional', Object.hasOwn(plain, '_read'), Object.keys(plain).join(','));
