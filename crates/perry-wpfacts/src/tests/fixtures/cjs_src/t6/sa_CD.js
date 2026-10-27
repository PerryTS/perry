// standalone control: requires only ./cd and runs it like run.js does (no computed-key lookup)
const b = require('./cd').newInstance();
const n = parseInt(process.argv[2], 10), inner = parseInt(process.argv[3], 10);
for (let i = 0; i < n; i += 1) {
  if (!b.innerBenchmarkLoop(inner)) { throw Error('Benchmark failed with incorrect result'); }
}
process.stdout.write('CD ok\n');
