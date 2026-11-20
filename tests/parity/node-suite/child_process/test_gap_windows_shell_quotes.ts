import {execSync, exec, spawnSync} from 'node:child_process';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const command = 'node -e "process.stdout.write(\'quoted-output\');process.stderr.write(\'quoted-error\')"';
  console.log('sync:', execSync(command, {encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe']}) === 'quoted-output');
  const shell = spawnSync(command, [], {shell: true, encoding: 'utf8'});
  console.log('spawn shell:', shell.status === 0 && shell.stdout === 'quoted-output' && shell.stderr === 'quoted-error');
  const custom = spawnSync(command, [], {shell: true, argv0: 'custom shell', encoding: 'utf8'});
  console.log('shell argv0:', custom.status === 0 && custom.stdout === 'quoted-output' && custom.stderr === 'quoted-error');
  await new Promise<void>((resolve, reject) => {
    exec(command, (error, stdout, stderr) => {
      if (error) { reject(error); return; }
      console.log('async:', stdout === 'quoted-output' && stderr === 'quoted-error');
      resolve();
    });
  });
}
