import cluster from 'node:cluster';
import { isAbsolute } from 'node:path';
import { realpathSync } from 'node:fs';

if (cluster.isPrimary) {
  const guard = setTimeout(() => process.exit(2), 15000);
  const worker = cluster.fork();
  worker.on('message', (ok: boolean) => console.log(ok));
  worker.on('error', () => process.exit(1));
  worker.on('exit', (code: number) => {
    console.log(code === 0);
    clearTimeout(guard);
  });
} else {
  process.send!(isAbsolute(process.execPath)
    && process.execPath === realpathSync(process.execPath)
    && process.argv[0] === process.execPath
    && (process.env.PERRY_TEST_CLUSTER_ARG === undefined
      || process.argv[2] === process.env.PERRY_TEST_CLUSTER_ARG));
  process.disconnect();
}
