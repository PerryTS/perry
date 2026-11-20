// All listed cases in this root-cause group. Oracle: .node-version.
import * as m2 from "node:crypto";
import * as m12 from "node:net";
import * as m16 from "node:stream/web";
import * as m21 from "node:async_hooks";
import { EventEmitter as EE } from "node:events";
import { Buffer } from "node:buffer";
// Case 9: node:crypto.`ECDH.convertKey` read as a value
console.log("case 9");
{
const f = m2.ECDH.convertKey;
console.log(typeof f);
try { console.log(f(Buffer.alloc(0), "prime256v1")); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 58: node:net.`BlockList.isBlockList` read as a value
console.log("case 58");
{
const f = m12.BlockList.isBlockList;
console.log(typeof f);
try { console.log(f({})); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 59: node:net.`SocketAddress.isSocketAddress` read as a value
console.log("case 59");
{
const f = m12.SocketAddress.isSocketAddress;
console.log(typeof f);
try { console.log(f({})); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 81: node:stream/web.`ReadableStream.from` read as a value
console.log("case 81");
{
const f = m16.ReadableStream.from;
console.log(typeof f);
try { console.log(typeof f([1,2])); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 114: node:async_hooks.saved `AsyncResource.bind`
console.log("case 114");
{
const f = m21.AsyncResource.bind;
try { console.log(f((n: number) => n * 3)(4)); } catch (e: any) { console.log(e.constructor.name); }
}

// Case 115: node:net.saved `SocketAddress.parse`
console.log("case 115");
{
const f = m12.SocketAddress.parse;
try { console.log(f("127.0.0.1:80")?.address); } catch (e: any) { console.log(e.constructor.name); }
}
console.log('positive own statics');
const isList = m12.BlockList.isBlockList;
console.log(isList(new m12.BlockList()));
const isAddress = m12.SocketAddress.isSocketAddress;
console.log(isAddress(new m12.SocketAddress()));
const parse = m12.SocketAddress.parse;
console.log(parse('invalid') === undefined);
console.log(parse('[::1]:443')?.address);
const convert = m2.ECDH.convertKey;
console.log(convert(Buffer.alloc(0), 'prime256v1') === '');
try { convert(Buffer.from([1]), 'prime256v1'); }
catch (error: any) { console.log(error.code); }
console.log(Object.hasOwn(m21.AsyncResource, 'bind'));
const from = m16.ReadableStream.from;
const source = from([1, 2]);
console.log(source instanceof m16.ReadableStream);
async function readSource() {
  const reader = source.getReader();
  console.log((await reader.read()).value);
  console.log((await reader.read()).value);
  console.log((await reader.read()).done);
}
readSource();

for (const arg of [undefined, null, 1, {}]) {
  try { m12.SocketAddress.parse(arg as any); console.log('parse accepted'); }
  catch (error) { console.log(error.name, error.code, error.message); }
}
console.log(m2.ECDH.convertKey('036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296', 'prime256v1', 'hex', 'hex', 'uncompressed'));

for (const [ctor, name] of [[m2.ECDH, 'convertKey'], [m12.BlockList, 'isBlockList'], [m12.SocketAddress, 'isSocketAddress'], [m12.SocketAddress, 'parse'], [m16.ReadableStream, 'from'], [m21.AsyncResource, 'bind']]) {
  const d = Object.getOwnPropertyDescriptor(ctor, name);
  console.log(name, d.writable, d.enumerable, d.configurable, d.value.length, Object.hasOwn(d.value, 'prototype'));
}

const savedParse = m12.SocketAddress.parse;
for (const arg of [undefined, null, 1, {}]) {
  try { savedParse(arg as any); console.log('saved parse accepted'); }
  catch (error) { console.log(error.name, error.code, error.message); }
}
