import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { load } from "./generated/bindings";
import { load as loadResources } from "./generated-resources/bindings";
import { load as loadResourceStream } from "./generated-resource-stream/bindings";
import type { ComponentAsyncType } from "perry/component";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
async function delay(): Promise<void> { await new Promise<void>(resolve => setTimeout(resolve, 1)); }
function type(component: any, kind: string, payloadKind: string): ComponentAsyncType {
  for (const factory of component.asyncTypes()) {
    const schema = JSON.parse(factory.schema);
    if (factory.kind === kind && (schema.value === null ? payloadKind === "void" : schema.value.kind === payloadKind)) return factory;
  }
  throw new Error("Missing factory: " + kind + "<" + payloadKind + ">");
}
const context = new AsyncLocalStorage<object>();
async function scalarCases(): Promise<void> {
  const component = load(readFileSync(process.argv[2]), {});
  const factory = type(component, "future", "tuple");
  (factory as any).schema = "tampered";
  (factory as any).kind = "stream";
  const pair = await factory.create<[bigint, string, Uint8Array]>();
  check(pair.kind === "future", "factory uses private nominal type after metadata mutation");
  if (pair.kind !== "future") throw new Error("wrong producer");
  const moved = component.exports.data(pair.value);
  let transferred = false;
  try { pair.value.read(); } catch (_) { transferred = true; }
  check(transferred, "lower receipt invalidates original future reader");
  const expected = {};
  context.enterWith(expected);
  const writing = pair.writer.write([18446744073709551615n, "héllo 🌍", new Uint8Array([0,255,17])]);
  const reading = moved.read();
  gc();
  const payload = await reading;
  const written = await writing;
  check(context.getStore() === expected, "producer preserves ALS through write/read overlap");
  check(payload[0] === 18446744073709551615n && payload[1] === "héllo 🌍", "wide integer and Unicode future payload");
  check(payload[2] instanceof Uint8Array && payload[2][1] === 255, "nested future byte payload");
  check(written.written === 1 && written.closed && written.remaining.length === 0, "future write completes exactly once");
  pair.writer.dispose();

  const strings = await type(component, "stream", "string").create<string>();
  if (strings.kind !== "stream") throw new Error("wrong stream producer");
  const stringReader = component.exports.strings(strings.value);
  const firstWrite = strings.writer.write(["α", "β", "γ"]);
  const first = await stringReader.read(1);
  const partial = await firstWrite;
  check(!first.done && first.value[0] === "α" && partial.written === 1 && partial.remaining.length === 2, "stream returns partial remainder");
  const retry = strings.writer.write(partial.remaining);
  const second = await stringReader.read(2);
  const retried = await retry;
  check(!second.done && second.value[0] === "β" && second.value[1] === "γ" && retried.written === 2, "partial remainder can be retried");
  strings.writer.close();
  check((await stringReader.read()).done, "writer close produces EOF");

  const bytes = await type(component, "stream", "u8").create<number, Uint8Array>();
  if (bytes.kind !== "stream") throw new Error("wrong byte producer");
  const byteReader = component.exports.bytes(bytes.value);
  const byteWrite = bytes.writer.write(new Uint8Array([0,255,17]));
  const byteChunk = await byteReader.read(2);
  const bytePartial = await byteWrite;
  check(!byteChunk.done && byteChunk.value instanceof Uint8Array && byteChunk.value[1] === 255, "byte stream uses Uint8Array chunks");
  check(bytePartial.remaining instanceof Uint8Array && bytePartial.remaining.length === 1 && bytePartial.remaining[0] === 17, "byte write remainder uses Uint8Array");
  const byteRetry = bytes.writer.write(bytePartial.remaining);
  const last = await byteReader.read(1);
  await byteRetry;
  check(!last.done && last.value[0] === 17, "byte remainder round trip");
  bytes.writer.dispose();
  check((await byteReader.read()).done, "byte stream EOF");

  const unitFuture = await type(component, "future", "void").create<void>();
  if (unitFuture.kind !== "future") throw new Error("wrong void producer");
  const voidReader = component.exports.void(unitFuture.value);
  const voidWrite = unitFuture.writer.write(undefined);
  check(await voidReader.read() === undefined, "payload-free future maps to undefined");
  check((await voidWrite).written === 1, "void future write count");
  const units = await type(component, "stream", "void").create<void>();
  if (units.kind !== "stream") throw new Error("wrong unit producer");
  const unitReader = component.exports.units(units.value);
  const unitWrite = units.writer.write([undefined, undefined, undefined]);
  const unitChunk = await unitReader.read(3);
  check(!unitChunk.done && unitChunk.value.length === 3 && unitChunk.value[0] === undefined && 0 in unitChunk.value, "unit stream materializes counted undefined elements");
  check((await unitWrite).written === 3, "unit stream preserves write count");
  units.writer.close();
  check((await unitReader.read()).done, "unit stream EOF");

  const outer = await type(component, "future", "future").create<any>();
  const inner = await type(component, "future", "u32").create<number>();
  if (outer.kind !== "future" || inner.kind !== "future") throw new Error("wrong nested producer");
  const outerReader = component.exports.nested(outer.value);
  const outerWrite = outer.writer.write(inner.value);
  const innerReader = await outerReader.read();
  await outerWrite;
  const innerWrite = inner.writer.write(73);
  check(await innerReader.read() === 73, "nested factory preserves both writable endpoints");
  await innerWrite;
  component.dispose();
}
async function resources(): Promise<void> {
  const component = loadResources(readFileSync(process.argv[3]), {});
  const data = component.exports["test:producer/data"];
  const pair = await component.asyncTypes()[0].create<any>();
  if (pair.kind !== "future") throw new Error("wrong resource future");
  const owned = await (await data.make()).read();
  const pending = pair.writer.write(owned);
  await delay();
  gc();
  pair.writer.cancel();
  const cancelled = await pending;
  check(cancelled.cancelled && cancelled.written === 0 && cancelled.remaining.length === 1, "cancellation returns unconsumed owned payload");
  check(data.drops() === 0, "cancelled payload remains owned");
  let stale = false;
  try { owned.dispose(); } catch (_) { stale = true; }
  check(stale, "lowered input owner cannot be reused");
  const take = data.take(pair.value);
  const retry = pair.writer.write(cancelled.remaining[0]);
  const received = await take;
  check((await retry).written === 1, "cancelled owner can be retried into an actual guest consumer");
  received.dispose();
  check(data.drops() === 1, "guest returns original nominal resource for exactly one destructor");
  component.dispose();

  const aborted = loadResources(readFileSync(process.argv[3]), {});
  const future = await aborted.asyncTypes()[0].create<any>();
  if (future.kind !== "future") throw new Error("wrong abort future");
  let rejected = false;
  const waiting = future.value.read().catch(() => { rejected = true; });
  future.writer.dispose();
  await waiting;
  check(rejected, "unfinished future disposal rejects pending reader and retires component");
  let retired = false;
  try { aborted.exports["test:producer/data"].drops(); } catch (_) { retired = true; }
  check(retired, "canonical future abandonment follows producer-abort policy");
  aborted.dispose();

  const unobserved = loadResources(readFileSync(process.argv[3]), {});
  const unused = await unobserved.asyncTypes()[0].create<any>();
  unused.writer.dispose();
  await delay();
  let quiet = false;
  try { unobserved.exports["test:producer/data"].drops(); } catch (_) { quiet = true; }
  check(quiet, "future disposal without an observer retires quietly");
  unobserved.dispose();
}
async function resourceStreamGc(): Promise<void> {
  const component = loadResourceStream(readFileSync(process.argv[4]), {});
  const data = component.exports["test:producer/data"];
  const pair = await component.asyncTypes()[0].create<any>();
  if (pair.kind !== "stream") throw new Error("wrong resource stream");
  const original = await data.make();
  const chunk = await original.read(1);
  check(!chunk.done, "source stream yields owned resource");
  if (chunk.done) throw new Error("empty source");
  check((await original.read()).done, "source stream reaches EOF");
  await (async () => { pair.writer.write([chunk.value[0]]); })();
  await delay();
  gc();
  await delay();
  gc();
  await delay();
  check(data.drops() === 1, "collecting an unobserved pending write disposes unconsumed owners");
  check((await pair.value.read()).done, "collected pending stream writer closes readable endpoint");
  const next = await data.make();
  const item = await next.read(1);
  if (item.done) throw new Error("empty reused source");
  item.value[0].dispose();
  check((await next.read()).done && data.drops() === 2, "component stays reusable after writer GC");
  component.dispose();
}
async function main(): Promise<void> {
  await scalarCases();
  await resources();
  await resourceStreamGc();
  console.log("PASS component producers: factories, ownership, partial writes, cancellation, GC");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
