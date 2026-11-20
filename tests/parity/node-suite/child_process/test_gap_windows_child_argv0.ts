import {spawnSync, execFileSync, execFile, spawn} from 'node:child_process';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const argv0 = 'custom argv0 euro-€';
  const args = ['-e', 'process.stdout.write(process.argv0)'];
  const result = spawnSync('node', args, {argv0, encoding: 'utf8'});
  console.log('spawnSync:', result.status === 0 && result.stdout === argv0);
  console.log('execFileSync:', execFileSync('node', args, {argv0, encoding: 'utf8'}) === argv0);
  const defaultArgv0 = spawnSync('node', args, {encoding: 'utf8'}).stdout;
  await new Promise<void>((resolve, reject) => {
    execFile('node', args, {argv0, encoding: 'utf8'}, (error, stdout) => {
      if (error) { reject(error); return; }
      console.log('execFile ignores argv0:', stdout === defaultArgv0);
      resolve();
    });
  });
  await new Promise<void>((resolve, reject) => {
    const child = spawn('node', args, {argv0});
    let text = '';
    child.stdout.on('data', data => { text += data.toString(); });
    child.on('error', reject);
    child.on('close', code => { console.log('spawn:', code === 0 && text === argv0); resolve(); });
  });
}
