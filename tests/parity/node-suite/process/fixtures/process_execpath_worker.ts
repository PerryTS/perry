import { parentPort } from 'node:worker_threads';
import { isAbsolute } from 'node:path';
import { realpathSync } from 'node:fs';

parentPort!.postMessage(isAbsolute(process.execPath)
  && process.execPath === realpathSync(process.execPath)
  && process.argv[0] === process.execPath);
