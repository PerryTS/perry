import {open, writeFile, unlink} from 'node:fs/promises';
async function main() {
  const path = 'objtables-gap-filehandle.tmp';
  await writeFile(path, 'owned-fd');
  const file = await open(path, 'r');
  for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
  console.log(await file.readFile('utf8'));
  await file.close();
  await unlink(path);
}
main();
