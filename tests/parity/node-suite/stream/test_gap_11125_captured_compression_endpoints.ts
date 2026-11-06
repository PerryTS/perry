function tryIt(label: string, f: () => unknown) {
  try { console.log(label, '->', typeof f()); }
  catch (e: any) { console.log(label, '-> THROWS', e.message); }
}
const cs1 = new CompressionStream('gzip');
tryIt('top-level getWriter', () => cs1.writable.getWriter());
function syncFn() {
 const cs2 = new CompressionStream('gzip');
 tryIt('sync fn getWriter', () => cs2.writable.getWriter());
}
syncFn();
async function asyncAny() {
 const fmt: any = 'gzip';
 const cs4 = new CompressionStream(fmt);
 tryIt('async any getWriter', () => cs4.writable.getWriter());
 tryIt('async any typeof writable', () => cs4.writable);
}
asyncAny();

function endpoints(label: string, stream: any) {
  const read = () => stream.readable;
  const write = () => stream.writable;
  tryIt(label + ' readable', read);
  tryIt(label + ' writable', write);
  tryIt(label + ' reader', () => read().getReader());
  tryIt(label + ' writer', () => write().getWriter());
}
endpoints('decompression', new DecompressionStream('gzip'));
endpoints('encoder', new TextEncoderStream());
endpoints('decoder', new TextDecoderStream());
