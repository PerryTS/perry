import { Socket, Server } from 'node:net';

// Names and constructor properties are user data, not native family facts.
Object.defineProperty(Socket, 'name', { value: 'Server', configurable: true });
Object.defineProperty(Server, 'name', { value: 'Socket', configurable: true });
const socketConstructor = Socket.prototype.constructor;
const serverConstructor = Server.prototype.constructor;
Socket.prototype.constructor = Server;
Server.prototype.constructor = Socket;

function DerivedSocket(this: any) { Socket.call(this); }
Object.setPrototypeOf(DerivedSocket.prototype, Socket.prototype);
function DerivedServer(this: any) { Reflect.apply(Server, this, []); }
Object.setPrototypeOf(DerivedServer.prototype, Server.prototype);
const socket = new (DerivedSocket as any)();
const server = new (DerivedServer as any)();
console.log('socket', typeof socket.write, typeof socket.connect, socket.destroyed);
console.log('server', typeof server.listen, typeof server.getConnections, server.listening);
const SocketAlias: any = Socket;
const ServerAlias: any = Server;
class ClassSocket extends SocketAlias { constructor() { super(); } }
class ClassServer extends ServerAlias { constructor() { super(); } }
const classSocket = new ClassSocket();
const classServer = new ClassServer();
console.log('class socket', typeof classSocket.write, classSocket.destroyed);
console.log('class server', typeof classServer.listen, classServer.listening);
classSocket.destroy();
socket.destroy();
console.log('destroyed', socket.destroyed);
Socket.prototype.constructor = socketConstructor;
Server.prototype.constructor = serverConstructor;
