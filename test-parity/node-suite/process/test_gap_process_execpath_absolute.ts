// Executable identity must be independent of the invocation spelling.
// tests/EXP1_process_execpath_absolute.py also launches this via relative,
// absolute, symlink and PATH names and checks argv0 against each launch name.
import proc, { execPath, argv0 } from 'node:process';
import { isAbsolute } from 'node:path';
import { realpathSync } from 'node:fs';

console.log(isAbsolute(process.execPath));
console.log(process.execPath === realpathSync(process.execPath));
console.log(process.argv[0] === process.execPath);
console.log(execPath === process.execPath);
console.log(proc.execPath === process.execPath);
console.log(globalThis.process.execPath === process.execPath);
const property = 'execPath';
console.log(proc[property] === process.execPath);
console.log(argv0 === process.argv0);
console.log(globalThis.process.argv0 === process.argv0);
const expectedArgv0 = process.env.PERRY_TEST_EXEC_ARGV0;
console.log(expectedArgv0 === undefined ? process.argv0.length > 0 : process.argv0 === expectedArgv0);
const expectedExecPath = process.env.PERRY_TEST_EXEC_PATH;
console.log(expectedExecPath === undefined || process.execPath === expectedExecPath);
