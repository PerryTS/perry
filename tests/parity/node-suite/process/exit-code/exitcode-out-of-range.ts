// #6666: an out-of-range integer exitCode is stored verbatim (getter returns
// 257) but reduced modulo 256 by the OS at exit, so the process status is 1.
process.exitCode = 257;
console.log("stored:", process.exitCode);
