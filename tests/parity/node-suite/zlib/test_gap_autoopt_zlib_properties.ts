// Dynamic properties must remain available when node:zlib is routed to the
// external provider and auto-optimize strips the bundled compression feature.
import { createGzip } from "node:zlib";

function inspect(stream: any) {
    console.log(typeof stream.write);
    console.log(typeof stream.end);
    console.log(typeof stream.on);
    console.log(stream.readableLength);
    console.log(stream.writableLength);
    console.log(stream.destroyed);
}

const gzip = createGzip();
inspect(gzip);
gzip.destroy();
