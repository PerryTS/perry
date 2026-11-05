// standalone control: requires ./cd and runs it like run.js does; one require by a computed
// specifier (the registry union's consumer, DECISIONS 171)
const b = require('./cd').newInstance();
const n = parseInt(process.argv[2], 10), inner = parseInt(process.argv[3], 10);
for (let i = 0; i < n; i += 1) {
  if (!b.innerBenchmarkLoop(inner)) { throw Error('Benchmark failed with incorrect result'); }
}
if (process.argv[4]) require(process.argv[4]);
process.stdout.write('CD ok\n');
