import * as crypto from 'node:crypto';
import * as stream from 'node:stream';
import * as net from 'node:net';
import * as http from 'node:http';
import * as tls from 'node:tls';
import { EventEmitter } from 'node:events';
console.log(Object.getPrototypeOf(stream.Readable) === stream.Stream);
console.log(Object.getPrototypeOf(stream.Writable) === stream.Stream);
console.log(Object.getPrototypeOf(stream.Duplex) === stream.Readable);
console.log(Object.getPrototypeOf(stream.Transform) === stream.Duplex);
console.log(Object.getPrototypeOf(stream.PassThrough) === stream.Transform);
console.log(Object.getPrototypeOf(net.Socket) === stream.Duplex);
console.log(Object.getPrototypeOf(http.Agent) === EventEmitter);
console.log(Object.getPrototypeOf(http.Server) === net.Server);
console.log(Object.getPrototypeOf(tls.Server) === net.Server);
console.log(Object.getPrototypeOf(tls.TLSSocket) === net.Socket);
console.log(net.Stream === net.Socket);
console.log(Object.getPrototypeOf(stream.Transform.prototype) === stream.Duplex.prototype);
const fake = Object.create(stream.Transform.prototype);
console.log(fake instanceof stream.Transform, fake instanceof stream.Duplex, fake instanceof EventEmitter);
class Child extends stream.Transform {
  constructor() { super(); }
  static receiver() { return this; }
}
class Grandchild extends Child { }
console.log(Grandchild.receiver() === Grandchild);
const instance = new Grandchild();
console.log(instance instanceof Grandchild, instance instanceof Child, instance instanceof stream.Transform, instance instanceof EventEmitter);
console.log(Child.getMaxListeners(new EventEmitter()));
// Verify ordinary inherited property lookup carries the original receiver.
(stream.Transform as any).identity = function() { return this; };
console.log((stream.PassThrough as any).identity() === stream.PassThrough);
console.log((Grandchild as any).identity() === Grandchild);
delete (stream.Transform as any).identity;
const before = EventEmitter.defaultMaxListeners;
stream.Readable.defaultMaxListeners = 23;
console.log(EventEmitter.defaultMaxListeners, stream.Writable.defaultMaxListeners);
console.log(Object.hasOwn(stream.Readable, 'defaultMaxListeners'));
EventEmitter.defaultMaxListeners = before;
console.log(stream.PassThrough.from === stream.Duplex.from);
console.log(stream.Transform.from === stream.Duplex.from);
console.log(stream.Readable.isDisturbed === stream.Stream.isDisturbed);
console.log(Object.hasOwn(stream.PassThrough, 'from'), Object.hasOwn(stream.Readable, 'isDisturbed'));

for (const ctor of [crypto.Hash, crypto.Hmac]) {
  const parent = Object.getPrototypeOf(ctor);
  console.log(ctor.prototype === parent.prototype, ctor.prototype.constructor === parent);
}

for (const ctor of [crypto.Hash, Object.getPrototypeOf(crypto.Hash), crypto.Hmac, Object.getPrototypeOf(crypto.Hmac), stream.Transform]) {
  const d = Object.getOwnPropertyDescriptor(ctor, 'prototype');
  const c = Object.getOwnPropertyDescriptor(ctor.prototype, 'constructor');
  console.log(ctor.name, d.writable, d.enumerable, d.configurable);
  console.log(c.writable, c.enumerable, c.configurable);
}
