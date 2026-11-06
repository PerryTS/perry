// Fetch Body ownership must survive moving the original bytes into .body.
const r = new Response('first body');
const body = r.body;
console.log('stable', r.body === body, r.bodyUsed);
const copy = r.clone();
console.log('original', await r.text(), r.bodyUsed);
console.log('clone', await copy.text(), copy.bodyUsed);
try { await r.arrayBuffer(); console.log('twice', false); } catch { console.log('twice', true); }
try { r.clone(); console.log('late clone', false); } catch { console.log('late clone', true); }
const other = new Response(new Uint8Array([10, 20, 30]));
const before = other.clone();
console.log('bytes', (await other.arrayBuffer()).byteLength, (await before.arrayBuffer()).byteLength);
const streamed = new Response('streamed');
const cloned = streamed.clone();
let total = 0;
for await (const bytes of streamed.body!) total += bytes.length;
console.log('stream', total, await cloned.text());
console.log('disturbed', streamed.bodyUsed);
try { streamed.clone(); console.log('stream clone', false); } catch { console.log('stream clone', true); }
try { await streamed.text(); console.log('stream text', false); } catch { console.log('stream text', true); }
const locked = new Response('locked');
const reader = locked.body!.getReader();
console.log('locked used', locked.bodyUsed);
try { locked.clone(); console.log('locked clone', false); } catch { console.log('locked clone', true); }
try { await locked.text(); console.log('locked text', false); } catch { console.log('locked text', true); }
console.log('still unused', locked.bodyUsed);
reader.releaseLock();
console.log('released', await locked.text());
const mutable = new Response(new Uint8Array([65, 66]));
mutable.body;
const isolated = mutable.clone();
const mutableReader = mutable.body!.getReader();
const packet = await mutableReader.read();
packet.value![0] = 90;
mutableReader.releaseLock();
console.log('clone bytes isolated', await isolated.text());
