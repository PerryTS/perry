// Test missing fs functions for Perry parity gap analysis
// Expected: all assertions pass under node --experimental-strip-types

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

// `'/tmp/perry_fs_test_' + Date.now()` is not a unique path: Date.now() has
// millisecond resolution, so two executions that start in the same millisecond
// pick the SAME directory — and this test ends by tearing its directory down
// with a recursive rmSync. One run then stats a file another run is deleting,
// which surfaced on Linux CI as
//   EACCES: Permission denied, stat '/tmp/perry_fs_test_<ms>/test.txt'
// and red-lit unrelated PRs at random (it failed one #6409 run and passed the
// next, on the same commit).
//
// mkdtempSync is the primitive for this: the kernel picks the suffix and
// creates the directory atomically, so no two runs can ever collide.
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perry_fs_test_'));

const testFile = path.join(tmpDir, 'test.txt');
const testContent = 'hello perry fs';
fs.writeFileSync(testFile, testContent);

// --- fs.statSync ---
const stat = fs.statSync(testFile);
console.log(stat.isFile()); // true
console.log(stat.isDirectory()); // false
console.log(stat.size > 0); // true

const dirStat = fs.statSync(tmpDir);
console.log(dirStat.isDirectory()); // true
console.log(dirStat.isFile()); // false

// --- fs.readdirSync ---
fs.writeFileSync(path.join(tmpDir, 'a.txt'), 'a');
fs.writeFileSync(path.join(tmpDir, 'b.txt'), 'b');
const entries = fs.readdirSync(tmpDir);
console.log(entries.length >= 3); // true (test.txt, a.txt, b.txt)
console.log(entries.includes('a.txt')); // true
console.log(entries.includes('b.txt')); // true

// --- fs.renameSync ---
const renamedFile = path.join(tmpDir, 'renamed.txt');
fs.renameSync(path.join(tmpDir, 'a.txt'), renamedFile);
console.log(fs.existsSync(renamedFile)); // true
console.log(fs.existsSync(path.join(tmpDir, 'a.txt'))); // false

// --- fs.copyFileSync ---
const copiedFile = path.join(tmpDir, 'copied.txt');
fs.copyFileSync(renamedFile, copiedFile);
console.log(fs.existsSync(copiedFile)); // true
const copiedContent = fs.readFileSync(copiedFile, 'utf-8');
console.log(copiedContent === 'a'); // true

// --- fs.accessSync ---
let accessOk = true;
try {
  fs.accessSync(testFile);
} catch {
  accessOk = false;
}
console.log(accessOk); // true

let accessBad = false;
try {
  fs.accessSync(path.join(tmpDir, 'nonexistent.txt'));
} catch {
  accessBad = true;
}
console.log(accessBad); // true

// --- fs.realpathSync ---
const realPath = fs.realpathSync(tmpDir);
console.log(typeof realPath === 'string'); // true
console.log(realPath.length > 0); // true

// --- fs.mkdtempSync ---
const tempDir = fs.mkdtempSync(path.join('/tmp', 'perry-mkdtemp-'));
console.log(fs.existsSync(tempDir)); // true
console.log(tempDir.includes('perry-mkdtemp-')); // true
fs.rmdirSync(tempDir);

// --- fs.readFile (callback) ---
const readFileCallbackDone: Promise<void> = new Promise((resolve, reject) => {
  fs.readFile(testFile, 'utf-8', (err: Error | null, data: string) => {
    if (err) {
      console.log(false); // should not error
      reject(err);
    } else {
      console.log(data === testContent); // true
      resolve();
    }
  });
});

// --- fs.promises.readFile ---
const promiseResult = await fs.promises.readFile(testFile, 'utf-8');
console.log(promiseResult === testContent); // true

// Wait for callback test
await readFileCallbackDone;

// --- fs.createReadStream / fs.createWriteStream ---
const streamOutFile = path.join(tmpDir, 'stream_out.txt');
const streamContent = 'stream test data';
await new Promise<void>((resolve, reject) => {
  const ws = fs.createWriteStream(streamOutFile);
  ws.write(streamContent);
  ws.end();
  ws.on('finish', () => resolve());
  ws.on('error', (e: Error) => reject(e));
});
console.log(fs.existsSync(streamOutFile)); // true

const readStreamData = await new Promise<string>((resolve, reject) => {
  let data = '';
  const rs = fs.createReadStream(streamOutFile, { encoding: 'utf-8' });
  rs.on('data', (chunk: string) => { data += chunk; });
  rs.on('end', () => resolve(data));
  rs.on('error', (e: Error) => reject(e));
});
console.log(readStreamData === streamContent); // true

// --- Cleanup ---
fs.rmSync(tmpDir, { recursive: true, force: true });
console.log("All fs gap tests passed!");

// Expected output:
// true
// false
// true
// true
// false
// true
// true
// true
// true
// false
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// All fs gap tests passed!
