import os from 'node:os';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  console.log('errno:', os.constants.errno.ECONNRESET, os.constants.errno.ETIMEDOUT);
  console.log('dlopen:', typeof os.constants.dlopen.RTLD_LAZY, typeof os.constants.dlopen.RTLD_NOW);
  console.log('credentials:', typeof process.getuid, typeof process.geteuid, typeof process.getgid, typeof process.getegid);
  console.log('release libUrl:', typeof process.release.libUrl);
  const report = process.report.getReport();
  console.log('report limits:', typeof report.userLimits);
}
