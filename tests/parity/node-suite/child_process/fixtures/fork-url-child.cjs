process.send(
  { argv: process.argv.slice(2), cwd: process.cwd() },
  () => process.disconnect(),
);
