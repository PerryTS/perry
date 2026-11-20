import * as http from 'node:http';
import * as stream from 'node:stream';
const Incoming: any = http.IncomingMessage;
function show(label: string, receiver: any) {
  console.log(label, !!receiver._readableState, receiver instanceof Incoming,
    receiver instanceof stream.Readable, receiver.complete, receiver.aborted);
}
const receiver: any = Object.create(Incoming.prototype);
console.log('call result', Incoming.call(receiver) === undefined);
show('call', receiver);
const applied: any = Object.create(Incoming.prototype);
console.log('apply result', Incoming.apply(applied, []) === undefined);
show('apply', applied);
const socket: any = { readableHighWaterMark: 1234 };
const withSocket: any = Object.create(Incoming.prototype);
Incoming.call(withSocket, socket);
console.log('socket', withSocket.socket === socket, withSocket._readableState.highWaterMark);
console.log('readingMore', receiver._readableState.readingMore);
console.log('fields', receiver.socket === undefined, receiver.client === undefined, receiver.httpVersion === null, receiver.method === null, receiver.url === '', receiver.rawHeaders.length, receiver.rawTrailers.length);
class Child extends Incoming {}
show('subclass', new Child());
try { Incoming.call({}); console.log('unrelated receiver initialized'); }
catch (e: any) { console.log('unrelated receiver', e.name); }
