import { Gzip, gunzipSync } from 'node:zlib';
import { Transform } from 'node:stream';

class Custom extends Gzip {
  calls = 0;
  _transform(chunk: any, encoding: any, callback: any) {
    this.calls++;
    super._transform(chunk, encoding, callback);
  }
  _flush(callback: any) {
    console.log('override flush');
    super._flush(callback);
  }
}
const codec = new Custom({ chunkSize: 1024 });
const output: any[] = [];
codec.on('data', (chunk: any) => output.push(chunk));
codec.on('end', () => console.log('override', codec.calls, gunzipSync(Buffer.concat(output)).toString()));
codec.write('first/');
codec.end('last');

const ordinary = new Transform({ transform(chunk: any, _encoding: any, callback: any) {
  callback(null, Buffer.from(chunk.toString().toUpperCase()));
} });
ordinary.on('data', (chunk: any) => console.log('ordinary', chunk.toString()));
ordinary.end('plain');
