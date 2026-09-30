import fs from 'node:fs';
import {realpath as promiseRealpath} from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const drive = path.parse(process.cwd()).root;
  console.log('root resolve:', path.resolve('/audit-root') === path.join(drive, 'audit-root'));
  console.log('win32 resolve:', path.win32.resolve('\\audit-root') === path.join(drive, 'audit-root'));
  console.log('namespace root:', path.toNamespacedPath('/audit-root') === '\\\\?\\' + path.resolve('/audit-root'));
  console.log('root file URL:', pathToFileURL('/audit-root').pathname === '/' + path.resolve('/audit-root').replaceAll('\\', '/'));
  console.log('realpath:', fs.realpathSync('.') === process.cwd());
  console.log('promise realpath:', await promiseRealpath('.') === process.cwd());
  await new Promise<void>((resolve, reject) => {
    fs.realpath('.', (error, result) => {
      if (error) { reject(error); return; }
      console.log('callback realpath:', result === process.cwd());
      resolve();
    });
  });
  const posixCwd = process.cwd().replaceAll('\\', '/').slice(process.cwd().replaceAll('\\', '/').indexOf('/'));
  console.log('posix resolve:', path.posix.resolve('relative') === path.posix.join(posixCwd, 'relative'));
  console.log('posix file URL:', pathToFileURL('relative', {windows: false}).pathname === path.posix.resolve('relative'));
}
