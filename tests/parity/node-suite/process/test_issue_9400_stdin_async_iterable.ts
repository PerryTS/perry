// #9400: `process.stdin` must be async-iterable.
//
// Node's stdin is a Readable, so it carries `Symbol.asyncIterator` and every
// "is this an async iterable?" probe a stream-consuming library runs succeeds.
// Perry's stdin object had no such symbol at all, so
// `for await (const chunk of process.stdin)` threw
// "TypeError: value is not iterable" — which is how
// `claude -p --input-format stream-json` produced zero bytes and still exited
// 0: its reader is literally `for await (let line of this.input)` over
// `process.stdin`.
//
// This file asserts the SHAPE only — it must not consume stdin, because the
// parity runner does not attach one. The end-to-end piped read is covered by
// the `await` role of test_issue_9399_stdin_data_chunk_bytes.ts.

const stdin = process.stdin as any;

console.log("asyncIterator:", typeof stdin[Symbol.asyncIterator]);
console.log("asyncIterator in stdin:", Symbol.asyncIterator in stdin);
console.log("not sync-iterable:", typeof stdin[Symbol.iterator]);

// The probe every stream-consuming library runs before accepting a source.
function isAsyncIterable(value: unknown): boolean {
  return (
    value != null &&
    typeof (value as any)[Symbol.asyncIterator] === "function"
  );
}
console.log("isAsyncIterable(stdin):", isAsyncIterable(stdin));
console.log("isAsyncIterable(null):", isAsyncIterable(null));
console.log("isAsyncIterable({}):", isAsyncIterable({}));

// Calling the method yields an iterator with a `next`, without reading.
const iterator = stdin[Symbol.asyncIterator]();
console.log("iterator typeof:", typeof iterator);
console.log("iterator.next typeof:", typeof iterator.next);

// The symbol must not leak into the enumerable string keys.
console.log(
  "no string key:",
  Object.keys(stdin).indexOf("Symbol(Symbol.asyncIterator)") === -1,
);
