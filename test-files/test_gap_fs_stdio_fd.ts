// fs.writeSync / fs.write on the standard descriptors (fd 0, 1 and 2).
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const fs = req('node:fs');
console.log('written', fs.writeSync(1, 'to fd 1\n'));
console.log('written', fs.writeSync(1, Buffer.from('buffer to fd 1\n')));
fs.writeSync(2, 'to fd 2\n');
fs.write(1, 'async to fd 1\n', (err, n) => {
  console.log('callback', err === null, n);
});
