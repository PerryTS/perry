// #5989: an expando property attached to a Web ReadableStream must read back.
//
// React's `renderToReadableStream` attaches its shell-ready promise to the
// stream it returns (`stream.allReady = promise`); Next.js destructures it
// back off the awaited render result and chains `.finally` on it. Perry
// stored the write (the stream-band arms in `js_put_value_set` /
// `js_object_set_field_by_name` route to the per-stream expando table), but
// every READ returned `undefined`: a stream handle is a raw finite f64 id, so
// `js_object_get_field_by_name`'s primitive-number guard (#2128) classified
// the receiver as a plain number and returned `undefined` before the
// dedicated stream arm could run. `undefined.finally` then 500'd every
// force-dynamic Next.js route.

const rs = new ReadableStream({
  start(c) {
    c.enqueue(new Uint8Array([1, 2, 3]));
    c.close();
  },
});

// Plain expando write + the three read shapes the bundles use.
(rs as any).allReady = Promise.resolve("AR");
console.log("typeof direct:", typeof (rs as any).allReady);

function readViaParam(s: any) {
  return s.allReady;
}
console.log("typeof via-fn:", typeof readViaParam(rs));

const { allReady } = rs as any;
console.log("typeof destructured:", typeof allReady);

// The read value is the SAME promise — resolves with the written value.
(rs as any).allReady.then((v: string) => console.log("value:", v));

// Non-promise expandos too, and unknown keys still miss.
(rs as any).marker = 42;
console.log("marker:", (rs as any).marker);
console.log("missing:", typeof (rs as any).nope);

// Stream still works as a stream after carrying expandos.
const reader = rs.getReader();
reader.read().then(({ value, done }) => {
  console.log("read:", done, value ? value.length : -1);
});
