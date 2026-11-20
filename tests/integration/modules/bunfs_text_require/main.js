import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

// Use the existing loader API so this test is independent of first-class
// import.meta.require lowering.
const load = createRequire(import.meta.url);
const filename = '/$bunfs/root/message.md';
const expected = '# Text fixture\n\nPlain Markdown, not rendered HTML: héllo.\n';
if (readFileSync(filename, 'utf8') !== expected) throw new Error('asset bytes missing');
const value = load(filename);
if (typeof value !== 'string' || value !== expected) throw new Error('incorrect text-loader value');
if (load.resolve(filename) !== filename) throw new Error('incorrect virtual resolution');
if (load('/$bunfs/root/empty.txt') !== '') throw new Error('empty text module missing');
const relative = createRequire('/$bunfs/root/entry.js');
if (relative('./sub/../message.md') !== expected) throw new Error('relative virtual resolution');
for (let i = 0; i < 8; i++) {
  globalThis.gc();
  if (load(filename) !== expected || value !== expected) throw new Error('cached text roots lost');
}
for (const missing of ['/$bunfs/root/missing.md', '/$bunfs/root/file-only.md']) {
  let caught = false;
  try { load(missing); } catch (error) { caught = error.code === 'MODULE_NOT_FOUND'; }
  if (!caught) throw new Error('unregistered/file-loader asset treated as text');
}
console.log('PASS embedded text require: exact bytes, cache, resolve, relative paths, GC, missing files');

