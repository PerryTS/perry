import {spawnSync, execFileSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const node = execFileSync('node', ['-p', 'process.execPath'], {encoding: 'utf8'}).trim();
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-exe-lookup-'));
  const argv0 = 'lookup argv0';
  const args = ['-e', 'console.log(require("path").basename(process.execPath) + ":" + process.argv0)'];
  try {
    // The extensionless copy is deliberately ignored by Node's PATH search.
    for (const name of ['probe', 'probe.com', 'probe.exe', 'probe.extra.com']) {
      fs.copyFileSync(node, path.join(dir, name));
    }
    const options = {argv0, cwd: dir, encoding: 'utf8'};
    const com = spawnSync('probe', args, options);
    console.log('com before exe:', com.status === 0 && com.stdout.trim() === 'probe.com:' + argv0);
    fs.unlinkSync(path.join(dir, 'probe.com'));
    const exe = spawnSync('probe', args, options);
    console.log('exe fallback:', exe.status === 0 && exe.stdout.trim() === 'probe.exe:' + argv0);
    const relative = spawnSync('.\\probe', args, options);
    console.log('relative fallback:', relative.status === 0 && relative.stdout.trim() === 'probe.exe:' + argv0);
    const absolute = spawnSync(path.join(dir, 'probe'), args, options);
    console.log('absolute skips bare:', absolute.status === 0 && absolute.stdout.trim() === 'probe.exe:' + argv0);
    fs.unlinkSync(path.join(dir, 'probe'));
    const absoluteFallback = spawnSync(path.join(dir, 'probe'), args, options);
    console.log('absolute fallback:', absoluteFallback.status === 0 && absoluteFallback.stdout.trim() === 'probe.exe:' + argv0);
    const trailingDot = spawnSync('probe.', args, options);
    console.log('trailing dot fallback:', trailingDot.status === 0 && trailingDot.stdout.trim() === 'probe.exe:' + argv0);
    const appended = spawnSync('probe.extra', args, options);
    console.log('append extension:', appended.status === 0 && appended.stdout.trim() === 'probe.extra.com:' + argv0);
    const literal = spawnSync('probe.exe', args, options);
    console.log('literal extension:', literal.status === 0 && literal.stdout.trim() === 'probe.exe:' + argv0);
  } finally {
    fs.rmSync(dir, {recursive: true, force: true});
  }
}
