import { createHash } from 'node:crypto';
import { createGunzip, createInflate, createInflateRaw, gzipSync, deflateSync, deflateRawSync, gunzipSync, inflateSync, inflateRawSync } from 'node:zlib';

const data = Buffer.alloc(65539);
for (let i = 0; i < data.length; i++) data[i] = (i * 37) % 251;
const gzip = gzipSync(data);
const zlib = deflateSync(data);
const raw = deflateRawSync(data);
function digest(value: Buffer): string {
    return createHash('sha256').update(value).digest('hex');
}
console.log(digest(data));
console.log(digest(gunzipSync(gzip)), digest(inflateSync(zlib)), digest(inflateRawSync(raw)));

async function streamed(make: () => any, input: Buffer, step: number): Promise<Buffer> {
    const stream = make();
    const output: Buffer[] = [];
    const done = new Promise<void>((resolve, reject) => {
        stream.on('data', (chunk: Buffer) => output.push(Buffer.from(chunk)));
        stream.on('end', resolve);
        stream.on('error', reject);
    });
    for (let at = 0; at < input.length; at += step) stream.write(input.subarray(at, at + step));
    stream.end();
    await done;
    return Buffer.concat(output);
}

for (const step of [1, 7, 127, 4096]) {
    console.log(step, digest(await streamed(() => createGunzip({ chunkSize: 64 }), gzip, step)),
        digest(await streamed(() => createInflate({ chunkSize: 64 }), zlib, step)),
        digest(await streamed(() => createInflateRaw({ chunkSize: 64 }), raw, step)));
}
const members = Buffer.concat([gzip, gzip, Buffer.alloc(19)]);
console.log('members', digest(gunzipSync(members)), digest(await streamed(() => createGunzip(), members, 7)));
for (const input of [gzip.subarray(0, gzip.length - 1), Buffer.from(gzip)]) {
    if (input.length === gzip.length) input[input.length - 8] ^= 1;
    try { gunzipSync(input); } catch (error: any) { console.log('sync error', error.code); }
    try { await streamed(() => createGunzip(), input, 7); } catch (error: any) { console.log('stream error', error.code); }
}
