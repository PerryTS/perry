// All listed cases in this root-cause group. Oracle: .node-version.
import * as m0 from "node:child_process";
import * as m1 from "node:cluster";
import * as m2 from "node:crypto";
import * as m3 from "node:dgram";
import * as m4 from "node:domain";
import * as m5 from "node:events";
import * as m6 from "node:fs";
import * as m7 from "node:http";
import * as m8 from "node:http2";
import * as m9 from "node:https";
import * as m10 from "node:inspector";
import * as m11 from "node:inspector/promises";
import * as m12 from "node:net";
import * as m13 from "node:readline/promises";
import * as m14 from "node:repl";
import * as m15 from "node:stream";
import * as m17 from "node:tls";
import * as m18 from "node:tty";
import * as m19 from "node:worker_threads";
import * as m20 from "node:zlib";
import { EventEmitter as EE } from "node:events";
import { Buffer } from "node:buffer";
// Case 1: node:child_process.`ChildProcess.getMaxListeners` read as a value
console.log("case 1");
{
const f = m0.ChildProcess.getMaxListeners;
console.log(Object.getPrototypeOf(m0.ChildProcess)?.name);
const instance = Object.create(m0.ChildProcess.prototype);
console.log(instance instanceof m0.ChildProcess, instance instanceof EE);
console.log(Object.hasOwn(m0.ChildProcess, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 2: node:child_process.`ChildProcess.defaultMaxListeners` getter
console.log("case 2");
{
console.log(m0.ChildProcess.defaultMaxListeners);
}

// Case 3: node:cluster.`Worker.getMaxListeners` read as a value
console.log("case 3");
{
const f = m1.Worker.getMaxListeners;
console.log(Object.getPrototypeOf(m1.Worker)?.name);
const instance = Object.create(m1.Worker.prototype);
console.log(instance instanceof m1.Worker, instance instanceof EE);
console.log(Object.hasOwn(m1.Worker, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 4: node:cluster.`Worker.defaultMaxListeners` getter
console.log("case 4");
{
console.log(m1.Worker.defaultMaxListeners);
}

// Case 5: node:crypto.`Cipheriv.getMaxListeners` read as a value
console.log("case 5");
{
const f = m2.Cipheriv.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Cipheriv)?.name);
const instance = Object.create(m2.Cipheriv.prototype);
console.log(instance instanceof m2.Cipheriv, instance instanceof EE);
console.log(Object.hasOwn(m2.Cipheriv, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 6: node:crypto.`Cipheriv.defaultMaxListeners` getter
console.log("case 6");
{
console.log(m2.Cipheriv.defaultMaxListeners);
}

// Case 7: node:crypto.`Decipheriv.getMaxListeners` read as a value
console.log("case 7");
{
const f = m2.Decipheriv.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Decipheriv)?.name);
const instance = Object.create(m2.Decipheriv.prototype);
console.log(instance instanceof m2.Decipheriv, instance instanceof EE);
console.log(Object.hasOwn(m2.Decipheriv, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 8: node:crypto.`Decipheriv.defaultMaxListeners` getter
console.log("case 8");
{
console.log(m2.Decipheriv.defaultMaxListeners);
}

// Case 10: node:crypto.`Hash.getMaxListeners` read as a value
console.log("case 10");
{
const f = m2.Hash.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Hash)?.name);
const instance = Object.create(m2.Hash.prototype);
console.log(instance instanceof m2.Hash, instance instanceof EE);
console.log(Object.hasOwn(m2.Hash, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 11: node:crypto.`Hash.defaultMaxListeners` getter
console.log("case 11");
{
console.log(m2.Hash.defaultMaxListeners);
}

// Case 12: node:crypto.`Hmac.getMaxListeners` read as a value
console.log("case 12");
{
const f = m2.Hmac.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Hmac)?.name);
const instance = Object.create(m2.Hmac.prototype);
console.log(instance instanceof m2.Hmac, instance instanceof EE);
console.log(Object.hasOwn(m2.Hmac, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 13: node:crypto.`Hmac.defaultMaxListeners` getter
console.log("case 13");
{
console.log(m2.Hmac.defaultMaxListeners);
}

// Case 14: node:crypto.`Sign.getMaxListeners` read as a value
console.log("case 14");
{
const f = m2.Sign.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Sign)?.name);
const instance = Object.create(m2.Sign.prototype);
console.log(instance instanceof m2.Sign, instance instanceof EE);
console.log(Object.hasOwn(m2.Sign, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 15: node:crypto.`Sign.defaultMaxListeners` getter
console.log("case 15");
{
console.log(m2.Sign.defaultMaxListeners);
}

// Case 16: node:crypto.`Verify.getMaxListeners` read as a value
console.log("case 16");
{
const f = m2.Verify.getMaxListeners;
console.log(Object.getPrototypeOf(m2.Verify)?.name);
const instance = Object.create(m2.Verify.prototype);
console.log(instance instanceof m2.Verify, instance instanceof EE);
console.log(Object.hasOwn(m2.Verify, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 17: node:crypto.`Verify.defaultMaxListeners` getter
console.log("case 17");
{
console.log(m2.Verify.defaultMaxListeners);
}

// Case 18: node:dgram.`Socket.getMaxListeners` read as a value
console.log("case 18");
{
const f = m3.Socket.getMaxListeners;
console.log(Object.getPrototypeOf(m3.Socket)?.name);
const instance = Object.create(m3.Socket.prototype);
console.log(instance instanceof m3.Socket, instance instanceof EE);
console.log(Object.hasOwn(m3.Socket, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 19: node:dgram.`Socket.defaultMaxListeners` getter
console.log("case 19");
{
console.log(m3.Socket.defaultMaxListeners);
}

// Case 20: node:domain.`Domain.getMaxListeners` read as a value
console.log("case 20");
{
const f = m4.Domain.getMaxListeners;
console.log(Object.getPrototypeOf(m4.Domain)?.name);
const instance = Object.create(m4.Domain.prototype);
console.log(instance instanceof m4.Domain, instance instanceof EE);
console.log(Object.hasOwn(m4.Domain, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 21: node:domain.`Domain.defaultMaxListeners` getter
console.log("case 21");
{
console.log(m4.Domain.defaultMaxListeners);
}

// Case 22: node:events.`EventEmitterAsyncResource.getMaxListeners` read as a value
console.log("case 22");
{
const f = m5.EventEmitterAsyncResource.getMaxListeners;
console.log(Object.getPrototypeOf(m5.EventEmitterAsyncResource)?.name);
const instance = Object.create(m5.EventEmitterAsyncResource.prototype);
console.log(instance instanceof m5.EventEmitterAsyncResource, instance instanceof EE);
console.log(Object.hasOwn(m5.EventEmitterAsyncResource, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 23: node:events.`EventEmitterAsyncResource.defaultMaxListeners` getter
console.log("case 23");
{
console.log(m5.EventEmitterAsyncResource.defaultMaxListeners);
}

// Case 24: node:fs.`ReadStream.getMaxListeners` read as a value
console.log("case 24");
{
const f = m6.ReadStream.getMaxListeners;
console.log(Object.getPrototypeOf(m6.ReadStream)?.name);
const instance = Object.create(m6.ReadStream.prototype);
console.log(instance instanceof m6.ReadStream, instance instanceof EE);
console.log(Object.hasOwn(m6.ReadStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 25: node:fs.`ReadStream.defaultMaxListeners` getter
console.log("case 25");
{
console.log(m6.ReadStream.defaultMaxListeners);
}

// Case 26: node:fs.`WriteStream.getMaxListeners` read as a value
console.log("case 26");
{
const f = m6.WriteStream.getMaxListeners;
console.log(Object.getPrototypeOf(m6.WriteStream)?.name);
const instance = Object.create(m6.WriteStream.prototype);
console.log(instance instanceof m6.WriteStream, instance instanceof EE);
console.log(Object.hasOwn(m6.WriteStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 27: node:fs.`WriteStream.defaultMaxListeners` getter
console.log("case 27");
{
console.log(m6.WriteStream.defaultMaxListeners);
}

// Case 28: node:fs.`FileReadStream.getMaxListeners` read as a value
console.log("case 28");
{
const f = m6.FileReadStream.getMaxListeners;
console.log(Object.getPrototypeOf(m6.FileReadStream)?.name);
const instance = Object.create(m6.FileReadStream.prototype);
console.log(instance instanceof m6.FileReadStream, instance instanceof EE);
console.log(Object.hasOwn(m6.FileReadStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 29: node:fs.`FileReadStream.defaultMaxListeners` getter
console.log("case 29");
{
console.log(m6.FileReadStream.defaultMaxListeners);
}

// Case 30: node:fs.`FileWriteStream.getMaxListeners` read as a value
console.log("case 30");
{
const f = m6.FileWriteStream.getMaxListeners;
console.log(Object.getPrototypeOf(m6.FileWriteStream)?.name);
const instance = Object.create(m6.FileWriteStream.prototype);
console.log(instance instanceof m6.FileWriteStream, instance instanceof EE);
console.log(Object.hasOwn(m6.FileWriteStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 31: node:fs.`FileWriteStream.defaultMaxListeners` getter
console.log("case 31");
{
console.log(m6.FileWriteStream.defaultMaxListeners);
}

// Case 32: node:fs.`Utf8Stream.getMaxListeners` read as a value
console.log("case 32");
{
const f = m6.Utf8Stream.getMaxListeners;
console.log(Object.getPrototypeOf(m6.Utf8Stream)?.name);
const instance = Object.create(m6.Utf8Stream.prototype);
console.log(instance instanceof m6.Utf8Stream, instance instanceof EE);
console.log(Object.hasOwn(m6.Utf8Stream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 33: node:fs.`Utf8Stream.defaultMaxListeners` getter
console.log("case 33");
{
console.log(m6.Utf8Stream.defaultMaxListeners);
}

// Case 34: node:http.`Agent.getMaxListeners` read as a value
console.log("case 34");
{
const f = m7.Agent.getMaxListeners;
console.log(Object.getPrototypeOf(m7.Agent)?.name);
const instance = Object.create(m7.Agent.prototype);
console.log(instance instanceof m7.Agent, instance instanceof EE);
console.log(Object.hasOwn(m7.Agent, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 35: node:http.`Agent.defaultMaxListeners` getter
console.log("case 35");
{
console.log(m7.Agent.defaultMaxListeners);
}

// Case 36: node:http.`ClientRequest.getMaxListeners` read as a value
console.log("case 36");
{
const f = m7.ClientRequest.getMaxListeners;
console.log(Object.getPrototypeOf(m7.ClientRequest)?.name);
const instance = Object.create(m7.ClientRequest.prototype);
console.log(instance instanceof m7.ClientRequest, instance instanceof EE);
console.log(Object.hasOwn(m7.ClientRequest, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 37: node:http.`ClientRequest.defaultMaxListeners` getter
console.log("case 37");
{
console.log(m7.ClientRequest.defaultMaxListeners);
}

// Case 38: node:http.`IncomingMessage.getMaxListeners` read as a value
console.log("case 38");
{
const f = m7.IncomingMessage.getMaxListeners;
console.log(Object.getPrototypeOf(m7.IncomingMessage)?.name);
const instance = Object.create(m7.IncomingMessage.prototype);
console.log(instance instanceof m7.IncomingMessage, instance instanceof EE);
console.log(Object.hasOwn(m7.IncomingMessage, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 39: node:http.`IncomingMessage.defaultMaxListeners` getter
console.log("case 39");
{
console.log(m7.IncomingMessage.defaultMaxListeners);
}

// Case 40: node:http.`OutgoingMessage.getMaxListeners` read as a value
console.log("case 40");
{
const f = m7.OutgoingMessage.getMaxListeners;
console.log(Object.getPrototypeOf(m7.OutgoingMessage)?.name);
const instance = Object.create(m7.OutgoingMessage.prototype);
console.log(instance instanceof m7.OutgoingMessage, instance instanceof EE);
console.log(Object.hasOwn(m7.OutgoingMessage, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 41: node:http.`OutgoingMessage.defaultMaxListeners` getter
console.log("case 41");
{
console.log(m7.OutgoingMessage.defaultMaxListeners);
}

// Case 42: node:http.`Server.getMaxListeners` read as a value
console.log("case 42");
{
const f = m7.Server.getMaxListeners;
console.log(Object.getPrototypeOf(m7.Server)?.name);
const instance = Object.create(m7.Server.prototype);
console.log(instance instanceof m7.Server, instance instanceof EE);
console.log(Object.hasOwn(m7.Server, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 43: node:http.`Server.defaultMaxListeners` getter
console.log("case 43");
{
console.log(m7.Server.defaultMaxListeners);
}

// Case 44: node:http.`ServerResponse.getMaxListeners` read as a value
console.log("case 44");
{
const f = m7.ServerResponse.getMaxListeners;
console.log(Object.getPrototypeOf(m7.ServerResponse)?.name);
const instance = Object.create(m7.ServerResponse.prototype);
console.log(instance instanceof m7.ServerResponse, instance instanceof EE);
console.log(Object.hasOwn(m7.ServerResponse, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 45: node:http.`ServerResponse.defaultMaxListeners` getter
console.log("case 45");
{
console.log(m7.ServerResponse.defaultMaxListeners);
}

// Case 46: node:http2.`Http2ServerRequest.getMaxListeners` read as a value
console.log("case 46");
{
const f = m8.Http2ServerRequest.getMaxListeners;
console.log(Object.getPrototypeOf(m8.Http2ServerRequest)?.name);
const instance = Object.create(m8.Http2ServerRequest.prototype);
console.log(instance instanceof m8.Http2ServerRequest, instance instanceof EE);
console.log(Object.hasOwn(m8.Http2ServerRequest, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 47: node:http2.`Http2ServerRequest.defaultMaxListeners` getter
console.log("case 47");
{
console.log(m8.Http2ServerRequest.defaultMaxListeners);
}

// Case 48: node:http2.`Http2ServerResponse.getMaxListeners` read as a value
console.log("case 48");
{
const f = m8.Http2ServerResponse.getMaxListeners;
console.log(Object.getPrototypeOf(m8.Http2ServerResponse)?.name);
const instance = Object.create(m8.Http2ServerResponse.prototype);
console.log(instance instanceof m8.Http2ServerResponse, instance instanceof EE);
console.log(Object.hasOwn(m8.Http2ServerResponse, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 49: node:http2.`Http2ServerResponse.defaultMaxListeners` getter
console.log("case 49");
{
console.log(m8.Http2ServerResponse.defaultMaxListeners);
}

// Case 50: node:https.`Agent.getMaxListeners` read as a value
console.log("case 50");
{
const f = m9.Agent.getMaxListeners;
console.log(Object.getPrototypeOf(m9.Agent)?.name);
const instance = Object.create(m9.Agent.prototype);
console.log(instance instanceof m9.Agent, instance instanceof EE);
console.log(Object.hasOwn(m9.Agent, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 51: node:https.`Agent.defaultMaxListeners` getter
console.log("case 51");
{
console.log(m9.Agent.defaultMaxListeners);
}

// Case 52: node:https.`Server.getMaxListeners` read as a value
console.log("case 52");
{
const f = m9.Server.getMaxListeners;
console.log(Object.getPrototypeOf(m9.Server)?.name);
const instance = Object.create(m9.Server.prototype);
console.log(instance instanceof m9.Server, instance instanceof EE);
console.log(Object.hasOwn(m9.Server, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 53: node:https.`Server.defaultMaxListeners` getter
console.log("case 53");
{
console.log(m9.Server.defaultMaxListeners);
}

// Case 54: node:inspector.`Session.getMaxListeners` read as a value
console.log("case 54");
{
const f = m10.Session.getMaxListeners;
console.log(Object.getPrototypeOf(m10.Session)?.name);
const instance = Object.create(m10.Session.prototype);
console.log(instance instanceof m10.Session, instance instanceof EE);
console.log(Object.hasOwn(m10.Session, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 55: node:inspector.`Session.defaultMaxListeners` getter
console.log("case 55");
{
console.log(m10.Session.defaultMaxListeners);
}

// Case 56: node:inspector/promises.`Session.getMaxListeners` read as a value
console.log("case 56");
{
const f = m11.Session.getMaxListeners;
console.log(Object.getPrototypeOf(m11.Session)?.name);
const instance = Object.create(m11.Session.prototype);
console.log(instance instanceof m11.Session, instance instanceof EE);
console.log(Object.hasOwn(m11.Session, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 57: node:inspector/promises.`Session.defaultMaxListeners` getter
console.log("case 57");
{
console.log(m11.Session.defaultMaxListeners);
}

// Case 60: node:net.`Server.getMaxListeners` read as a value
console.log("case 60");
{
const f = m12.Server.getMaxListeners;
console.log(Object.getPrototypeOf(m12.Server)?.name);
const instance = Object.create(m12.Server.prototype);
console.log(instance instanceof m12.Server, instance instanceof EE);
console.log(Object.hasOwn(m12.Server, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 61: node:net.`Server.defaultMaxListeners` getter
console.log("case 61");
{
console.log(m12.Server.defaultMaxListeners);
}

// Case 62: node:net.`Socket.getMaxListeners` read as a value
console.log("case 62");
{
const f = m12.Socket.getMaxListeners;
console.log(Object.getPrototypeOf(m12.Socket)?.name);
const instance = Object.create(m12.Socket.prototype);
console.log(instance instanceof m12.Socket, instance instanceof EE);
console.log(Object.hasOwn(m12.Socket, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 63: node:net.`Socket.defaultMaxListeners` getter
console.log("case 63");
{
console.log(m12.Socket.defaultMaxListeners);
}

// Case 64: node:net.`Stream.getMaxListeners` read as a value
console.log("case 64");
{
const f = m12.Stream.getMaxListeners;
console.log(Object.getPrototypeOf(m12.Stream)?.name);
const instance = Object.create(m12.Stream.prototype);
console.log(instance instanceof m12.Stream, instance instanceof EE);
console.log(Object.hasOwn(m12.Stream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 65: node:net.`Stream.defaultMaxListeners` getter
console.log("case 65");
{
console.log(m12.Stream.defaultMaxListeners);
}

// Case 66: node:readline/promises.`Interface.getMaxListeners` read as a value
console.log("case 66");
{
const f = m13.Interface.getMaxListeners;
console.log(Object.getPrototypeOf(m13.Interface)?.name);
const instance = Object.create(m13.Interface.prototype);
console.log(instance instanceof m13.Interface, instance instanceof EE);
console.log(Object.hasOwn(m13.Interface, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 67: node:readline/promises.`Interface.defaultMaxListeners` getter
console.log("case 67");
{
console.log(m13.Interface.defaultMaxListeners);
}

// Case 68: node:repl.`REPLServer.getMaxListeners` read as a value
console.log("case 68");
{
const f = m14.REPLServer.getMaxListeners;
console.log(Object.getPrototypeOf(m14.REPLServer)?.name);
const instance = Object.create(m14.REPLServer.prototype);
console.log(instance instanceof m14.REPLServer, instance instanceof EE);
console.log(Object.hasOwn(m14.REPLServer, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 69: node:repl.`REPLServer.defaultMaxListeners` getter
console.log("case 69");
{
console.log(m14.REPLServer.defaultMaxListeners);
}

// Case 70: node:repl.`Recoverable.isError` read as a value
console.log("case 70");
{
const f = m14.Recoverable.isError;
console.log(typeof f);
try { console.log(f(new Error("x"))); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 71: node:stream.`Readable.getMaxListeners` read as a value
console.log("case 71");
{
const f = m15.Readable.getMaxListeners;
console.log(Object.getPrototypeOf(m15.Readable)?.name);
const instance = Object.create(m15.Readable.prototype);
console.log(instance instanceof m15.Readable, instance instanceof EE);
console.log(Object.hasOwn(m15.Readable, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 72: node:stream.`Readable.defaultMaxListeners` getter
console.log("case 72");
{
console.log(m15.Readable.defaultMaxListeners);
}

// Case 73: node:stream.`Writable.getMaxListeners` read as a value
console.log("case 73");
{
const f = m15.Writable.getMaxListeners;
console.log(Object.getPrototypeOf(m15.Writable)?.name);
const instance = Object.create(m15.Writable.prototype);
console.log(instance instanceof m15.Writable, instance instanceof EE);
console.log(Object.hasOwn(m15.Writable, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 74: node:stream.`Writable.defaultMaxListeners` getter
console.log("case 74");
{
console.log(m15.Writable.defaultMaxListeners);
}

// Case 75: node:stream.`Duplex.getMaxListeners` read as a value
console.log("case 75");
{
const f = m15.Duplex.getMaxListeners;
console.log(Object.getPrototypeOf(m15.Duplex)?.name);
const instance = Object.create(m15.Duplex.prototype);
console.log(instance instanceof m15.Duplex, instance instanceof EE);
console.log(Object.hasOwn(m15.Duplex, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 76: node:stream.`Duplex.defaultMaxListeners` getter
console.log("case 76");
{
console.log(m15.Duplex.defaultMaxListeners);
}

// Case 77: node:stream.`Transform.getMaxListeners` read as a value
console.log("case 77");
{
const f = m15.Transform.getMaxListeners;
console.log(Object.getPrototypeOf(m15.Transform)?.name);
const instance = Object.create(m15.Transform.prototype);
console.log(instance instanceof m15.Transform, instance instanceof EE);
console.log(Object.hasOwn(m15.Transform, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 78: node:stream.`Transform.defaultMaxListeners` getter
console.log("case 78");
{
console.log(m15.Transform.defaultMaxListeners);
}

// Case 79: node:stream.`PassThrough.getMaxListeners` read as a value
console.log("case 79");
{
const f = m15.PassThrough.getMaxListeners;
console.log(Object.getPrototypeOf(m15.PassThrough)?.name);
const instance = Object.create(m15.PassThrough.prototype);
console.log(instance instanceof m15.PassThrough, instance instanceof EE);
console.log(Object.hasOwn(m15.PassThrough, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 80: node:stream.`PassThrough.defaultMaxListeners` getter
console.log("case 80");
{
console.log(m15.PassThrough.defaultMaxListeners);
}

// Case 82: node:tls.`TLSSocket.getMaxListeners` read as a value
console.log("case 82");
{
const f = m17.TLSSocket.getMaxListeners;
console.log(Object.getPrototypeOf(m17.TLSSocket)?.name);
const instance = Object.create(m17.TLSSocket.prototype);
console.log(instance instanceof m17.TLSSocket, instance instanceof EE);
console.log(Object.hasOwn(m17.TLSSocket, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 83: node:tls.`TLSSocket.defaultMaxListeners` getter
console.log("case 83");
{
console.log(m17.TLSSocket.defaultMaxListeners);
}

// Case 84: node:tls.`Server.getMaxListeners` read as a value
console.log("case 84");
{
const f = m17.Server.getMaxListeners;
console.log(Object.getPrototypeOf(m17.Server)?.name);
const instance = Object.create(m17.Server.prototype);
console.log(instance instanceof m17.Server, instance instanceof EE);
console.log(Object.hasOwn(m17.Server, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 85: node:tls.`Server.defaultMaxListeners` getter
console.log("case 85");
{
console.log(m17.Server.defaultMaxListeners);
}

// Case 86: node:tty.`ReadStream.getMaxListeners` read as a value
console.log("case 86");
{
const f = m18.ReadStream.getMaxListeners;
console.log(Object.getPrototypeOf(m18.ReadStream)?.name);
const instance = Object.create(m18.ReadStream.prototype);
console.log(instance instanceof m18.ReadStream, instance instanceof EE);
console.log(Object.hasOwn(m18.ReadStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 87: node:tty.`ReadStream.defaultMaxListeners` getter
console.log("case 87");
{
console.log(m18.ReadStream.defaultMaxListeners);
}

// Case 88: node:tty.`WriteStream.getMaxListeners` read as a value
console.log("case 88");
{
const f = m18.WriteStream.getMaxListeners;
console.log(Object.getPrototypeOf(m18.WriteStream)?.name);
const instance = Object.create(m18.WriteStream.prototype);
console.log(instance instanceof m18.WriteStream, instance instanceof EE);
console.log(Object.hasOwn(m18.WriteStream, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 89: node:tty.`WriteStream.defaultMaxListeners` getter
console.log("case 89");
{
console.log(m18.WriteStream.defaultMaxListeners);
}

// Case 90: node:worker_threads.`Worker.getMaxListeners` read as a value
console.log("case 90");
{
const f = m19.Worker.getMaxListeners;
console.log(Object.getPrototypeOf(m19.Worker)?.name);
const instance = Object.create(m19.Worker.prototype);
console.log(instance instanceof m19.Worker, instance instanceof EE);
console.log(Object.hasOwn(m19.Worker, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 91: node:worker_threads.`Worker.defaultMaxListeners` getter
console.log("case 91");
{
console.log(m19.Worker.defaultMaxListeners);
}

// Case 92: node:zlib.`Deflate.getMaxListeners` read as a value
console.log("case 92");
{
const f = m20.Deflate.getMaxListeners;
console.log(Object.getPrototypeOf(m20.Deflate)?.name);
const instance = Object.create(m20.Deflate.prototype);
console.log(instance instanceof m20.Deflate, instance instanceof EE);
console.log(Object.hasOwn(m20.Deflate, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 93: node:zlib.`Deflate.defaultMaxListeners` getter
console.log("case 93");
{
console.log(m20.Deflate.defaultMaxListeners);
}

// Case 94: node:zlib.`Inflate.getMaxListeners` read as a value
console.log("case 94");
{
const f = m20.Inflate.getMaxListeners;
console.log(Object.getPrototypeOf(m20.Inflate)?.name);
const instance = Object.create(m20.Inflate.prototype);
console.log(instance instanceof m20.Inflate, instance instanceof EE);
console.log(Object.hasOwn(m20.Inflate, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 95: node:zlib.`Inflate.defaultMaxListeners` getter
console.log("case 95");
{
console.log(m20.Inflate.defaultMaxListeners);
}

// Case 96: node:zlib.`Gzip.getMaxListeners` read as a value
console.log("case 96");
{
const f = m20.Gzip.getMaxListeners;
console.log(Object.getPrototypeOf(m20.Gzip)?.name);
const instance = Object.create(m20.Gzip.prototype);
console.log(instance instanceof m20.Gzip, instance instanceof EE);
console.log(Object.hasOwn(m20.Gzip, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 97: node:zlib.`Gzip.defaultMaxListeners` getter
console.log("case 97");
{
console.log(m20.Gzip.defaultMaxListeners);
}

// Case 98: node:zlib.`Gunzip.getMaxListeners` read as a value
console.log("case 98");
{
const f = m20.Gunzip.getMaxListeners;
console.log(Object.getPrototypeOf(m20.Gunzip)?.name);
const instance = Object.create(m20.Gunzip.prototype);
console.log(instance instanceof m20.Gunzip, instance instanceof EE);
console.log(Object.hasOwn(m20.Gunzip, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 99: node:zlib.`Gunzip.defaultMaxListeners` getter
console.log("case 99");
{
console.log(m20.Gunzip.defaultMaxListeners);
}

// Case 100: node:zlib.`DeflateRaw.getMaxListeners` read as a value
console.log("case 100");
{
const f = m20.DeflateRaw.getMaxListeners;
console.log(Object.getPrototypeOf(m20.DeflateRaw)?.name);
const instance = Object.create(m20.DeflateRaw.prototype);
console.log(instance instanceof m20.DeflateRaw, instance instanceof EE);
console.log(Object.hasOwn(m20.DeflateRaw, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 101: node:zlib.`DeflateRaw.defaultMaxListeners` getter
console.log("case 101");
{
console.log(m20.DeflateRaw.defaultMaxListeners);
}

// Case 102: node:zlib.`InflateRaw.getMaxListeners` read as a value
console.log("case 102");
{
const f = m20.InflateRaw.getMaxListeners;
console.log(Object.getPrototypeOf(m20.InflateRaw)?.name);
const instance = Object.create(m20.InflateRaw.prototype);
console.log(instance instanceof m20.InflateRaw, instance instanceof EE);
console.log(Object.hasOwn(m20.InflateRaw, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 103: node:zlib.`InflateRaw.defaultMaxListeners` getter
console.log("case 103");
{
console.log(m20.InflateRaw.defaultMaxListeners);
}

// Case 104: node:zlib.`Unzip.getMaxListeners` read as a value
console.log("case 104");
{
const f = m20.Unzip.getMaxListeners;
console.log(Object.getPrototypeOf(m20.Unzip)?.name);
const instance = Object.create(m20.Unzip.prototype);
console.log(instance instanceof m20.Unzip, instance instanceof EE);
console.log(Object.hasOwn(m20.Unzip, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 105: node:zlib.`Unzip.defaultMaxListeners` getter
console.log("case 105");
{
console.log(m20.Unzip.defaultMaxListeners);
}

// Case 106: node:zlib.`BrotliCompress.getMaxListeners` read as a value
console.log("case 106");
{
const f = m20.BrotliCompress.getMaxListeners;
console.log(Object.getPrototypeOf(m20.BrotliCompress)?.name);
const instance = Object.create(m20.BrotliCompress.prototype);
console.log(instance instanceof m20.BrotliCompress, instance instanceof EE);
console.log(Object.hasOwn(m20.BrotliCompress, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 107: node:zlib.`BrotliCompress.defaultMaxListeners` getter
console.log("case 107");
{
console.log(m20.BrotliCompress.defaultMaxListeners);
}

// Case 108: node:zlib.`BrotliDecompress.getMaxListeners` read as a value
console.log("case 108");
{
const f = m20.BrotliDecompress.getMaxListeners;
console.log(Object.getPrototypeOf(m20.BrotliDecompress)?.name);
const instance = Object.create(m20.BrotliDecompress.prototype);
console.log(instance instanceof m20.BrotliDecompress, instance instanceof EE);
console.log(Object.hasOwn(m20.BrotliDecompress, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 109: node:zlib.`BrotliDecompress.defaultMaxListeners` getter
console.log("case 109");
{
console.log(m20.BrotliDecompress.defaultMaxListeners);
}

// Case 110: node:zlib.`ZstdCompress.getMaxListeners` read as a value
console.log("case 110");
{
const f = m20.ZstdCompress.getMaxListeners;
console.log(Object.getPrototypeOf(m20.ZstdCompress)?.name);
const instance = Object.create(m20.ZstdCompress.prototype);
console.log(instance instanceof m20.ZstdCompress, instance instanceof EE);
console.log(Object.hasOwn(m20.ZstdCompress, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 111: node:zlib.`ZstdCompress.defaultMaxListeners` getter
console.log("case 111");
{
console.log(m20.ZstdCompress.defaultMaxListeners);
}

// Case 112: node:zlib.`ZstdDecompress.getMaxListeners` read as a value
console.log("case 112");
{
const f = m20.ZstdDecompress.getMaxListeners;
console.log(Object.getPrototypeOf(m20.ZstdDecompress)?.name);
const instance = Object.create(m20.ZstdDecompress.prototype);
console.log(instance instanceof m20.ZstdDecompress, instance instanceof EE);
console.log(Object.hasOwn(m20.ZstdDecompress, "getMaxListeners"));
console.log(typeof f);
try { console.log(f(new EE())); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 113: node:zlib.`ZstdDecompress.defaultMaxListeners` getter
console.log("case 113");
{
console.log(m20.ZstdDecompress.defaultMaxListeners);
}
