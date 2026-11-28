// A failed chdir names the cwd as `path` and the target as `dest`, with the
// fields in Node's uvException order.
try {
  process.chdir("/perry-parity-definitely-missing-dir");
} catch (error: any) {
  console.log(Object.keys(error).join(","), error.code, error.syscall, error.dest);
  console.log(error.path === process.cwd(), typeof error.errno);
}
