// Native reads must publish the actual decoded length, including split input,
// invalid UTF-8, descriptor position and every supported encoding.
import * as fs from 'node:fs';
import * as fsp from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-read-direct-'));
const file = path.join(dir, 'data');
function units(value: string): string {
  let out = '';
  for (let i = 0; i < value.length; i++) out += (i ? ',' : '') + value.charCodeAt(i);
  return out;
}
async function main(): Promise<void> {
  for (const hex of ['', '610062', 'efbbbf41c3a9f09f9982', 'eda080edb080ff', 'f09f99', 'e2825880e282']) {
    fs.writeFileSync(file, Buffer.from(hex, 'hex'));
    console.log('utf8', units(fs.readFileSync(file, 'utf8')));
    console.log('options', units(fs.readFileSync(file, { encoding: 'UTF-8' })));
    console.log('promise', units(await fsp.readFile(file, 'utf8')));
    await new Promise<void>((resolve, reject) => {
      fs.readFile(file, 'utf8', (error: any, data: string) => {
        if (error) { reject(error); return; }
        console.log('callback', units(data)); resolve();
      });
    });
  }
  for (let split = 0; split < 4; split++) {
    const prefix = 'a'.repeat(65536 - split);
    fs.writeFileSync(file, prefix + '🙂é' + 'z'.repeat(65537));
    const value = fs.readFileSync(file, 'utf8');
    console.log('chunk', value.length, units(value.slice(prefix.length, prefix.length + 3)), value.charCodeAt(value.length - 1));
  }
  fs.writeFileSync(file, Buffer.from('00e941ff80', 'hex'));
  for (const encoding of ['hex', 'base64', 'base64url', 'ascii', 'latin1', 'binary']) {
    console.log('encoding', encoding, units(fs.readFileSync(file, encoding as any)));
  }
  fs.writeFileSync(file, Buffer.from('00d841003dd842de00dcff', 'hex'));
  for (const encoding of ['utf16le', 'utf-16le', 'ucs2', 'ucs-2']) {
    console.log('utf16', encoding, units(fs.readFileSync(file, encoding as any)));
  }
  const orderOptions = { get encoding() { console.log('encoding-getter'); throw new Error('encoding-failure'); } };
  try { fs.readFileSync(null as any, orderOptions as any); }
  catch (error: any) { console.log('getter-order', error.message); }
  fs.writeFileSync(file, 'abcdef');
  let getterReads = 0;
  const getterOptions = { get encoding() { getterReads++; return 'utf8'; } };
  // Check observable decoding and accessor dispatch without pinning Node's
  // implementation-dependent number of encoding probes.
  console.log('getter-result', fs.readFileSync(file, getterOptions), getterReads > 0);
  const fd = fs.openSync(file, 'r');
  const one = Buffer.alloc(1);
  fs.readSync(fd, one, 0, 1, null);
  console.log('fd', fs.readFileSync(fd, 'utf8'), fs.readFileSync(fd, 'utf8').length);
  fs.closeSync(fd);
  console.log('buffer', Buffer.isBuffer(fs.readFileSync(file)), fs.readFileSync(file).toString('hex'));
  const options = { encoding: 'utf8', flag: 'w+' };
  console.log('truncate', fs.readFileSync(file, options as any).length, fs.statSync(file).size);
  for (const target of [path.join(dir, 'missing'), dir]) {
    try { fs.readFileSync(target, 'utf8'); }
    catch (error: any) { console.log('error', error.code, error.syscall); }
  }
  for (const encoding of ['not-an-encoding', 'buffer', 'BUFFER', { encoding: 42 }]) {
    try { fs.readFileSync(file, encoding as any); }
    catch (error: any) { console.log('encoding-error', error.code); }
  }
  for (const encoding of ['', { encoding: '' }, { encoding: false }, { encoding: null }]) {
    console.log('no-encoding', Buffer.isBuffer(fs.readFileSync(file, encoding as any)));
  }
  for (const encoding of ['not-an-encoding', { encoding: 42 }]) {
    let pending: any;
    try { pending = fsp.readFile(file, encoding as any); console.log('promise-return', typeof pending.then); }
    catch (error: any) { console.log('promise-sync-error', error.code); }
    if (pending) {
      try { await pending; }
      catch (error: any) { console.log('promise-encoding-error', error.code); }
    }
  }
  fs.rmSync(dir, { recursive: true });
}
main().catch((error: any) => { console.log(error.code || error.message); process.exitCode = 1; });
