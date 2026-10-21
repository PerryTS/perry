import { Socket, Stream as NodeStream } from "node:net";
import { Socket as PlainSocket, Stream as PlainStream } from "net";
import * as nodeNet from "node:net";
import * as plainNet from "net";
import { EventEmitter as ForeignSocket } from "node:events";

const ValueSocket = nodeNet.Socket;
const ValueStream = plainNet.Stream;
function inspect(label: string, make: () => any) {
  try {
    const socket = make();
    console.log(label, socket.connecting, socket.pending, socket.destroyed,
      socket.readyState, typeof socket.address, socket instanceof nodeNet.Socket,
      Object.getPrototypeOf(socket) === nodeNet.Socket.prototype);
    console.log(label + " initial", socket.bufferSize, JSON.stringify(socket.address()));
    console.log(label + " cork", socket.cork(), socket.uncork());
    socket.destroy();
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.message);
  }
}
inspect("named", () => new Socket());
inspect("renamed", () => new PlainSocket());
inspect("node-stream", () => new NodeStream());
inspect("plain-stream", () => new PlainStream());
inspect("node-namespace", () => new nodeNet.Socket());
inspect("plain-namespace", () => new plainNet.Socket());
inspect("value-socket", () => new ValueSocket());
inspect("value-stream", () => new ValueStream());
function localClass() {
  class Socket { marker = "local"; }
  console.log("local-class", new Socket().marker);
}
function lexicalValue() {
  const Socket = class { marker = "lexical"; };
  console.log("lexical-value", new Socket().marker);
}
localClass();
lexicalValue();
const foreign = new ForeignSocket();
console.log("foreign-module", typeof foreign.emit, foreign instanceof nodeNet.Socket);
console.log("exports", nodeNet.Socket === plainNet.Socket,
  nodeNet.Stream === plainNet.Stream, nodeNet.Stream === nodeNet.Socket);
