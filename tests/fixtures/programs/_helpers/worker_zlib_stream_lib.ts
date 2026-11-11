// Streaming gunzip the way upm unpacks a tarball: an async generator piped
// through Readable.from into createGunzip, read with for await.
const stream = (globalThis as any).process.getBuiltinModule("node:stream");
const zlib = (globalThis as any).process.getBuiltinModule("node:zlib");

export function payload(seed: number): Uint8Array {
    const raw = new Uint8Array(300000);
    for (let i = 0; i < raw.length; i++) raw[i] = (i * seed) % 251;
    return zlib.gzipSync(raw);
}

export async function inflatedBytes(gz: Uint8Array): Promise<number> {
    async function* source(): AsyncGenerator<Uint8Array> {
        yield gz.subarray(0, 700);
        await new Promise((resolve) => setTimeout(resolve, 5));
        yield gz.subarray(700);
    }
    const input = stream.Readable.from(source());
    const gunzip = zlib.createGunzip({ chunkSize: 65536, readableHighWaterMark: 1 << 20 } as object);
    input.on("error", (error: Error) => gunzip.destroy(error));
    let total = 0;
    for await (const chunk of input.pipe(gunzip) as AsyncIterable<Uint8Array>) total += chunk.length;
    return total;
}
