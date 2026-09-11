import { readFileSync } from 'node:fs';
const source = readFileSync(process.argv[2], 'utf8');
const operation = process.argv[3];
const dynamicSpacer: any = Number(process.argv[7] || '0');
const iterations = Number(process.argv[4]);
const warmup = Number(process.argv[5]);
const input: any = JSON.parse('{"data":' + source + '}').data;
const keys: string[] = ['id', 'name', 'email', 'active', 'score', 'tags'];
let last: any = null;
function identity(key: string, value: any): any { return value; }
function run(count: number): number {
    let sum = 0;
    if (operation === 'plain') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input);
            sum += result.length; last = result;
        }
    } else if (operation === 'dynamic-zero') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, null, dynamicSpacer);
            sum += result.length; last = result;
        }
    } else if (operation === 'zero') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, null, 0);
            sum += result.length; last = result;
        }
    } else if (operation === 'pretty') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, null, 2);
            sum += result.length; last = result;
        }
    } else if (operation === 'keys') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, keys);
            sum += result.length; last = result;
        }
    } else if (operation === 'callback') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, identity);
            sum += result.length; last = result;
        }
    }
    return sum;
}
let checksum = run(warmup);
const rssBefore = process.memoryUsage().rss;
const cpuBefore = process.cpuUsage();
const started = performance.now();
checksum += run(iterations);
const elapsed = performance.now() - started;
const cpuAfter = process.cpuUsage();
console.log('RESULT', elapsed, cpuAfter.user - cpuBefore.user,
    cpuAfter.system - cpuBefore.system, rssBefore, process.memoryUsage().rss, checksum, 0);
if (process.argv[6] === 'verify') console.log('VERIFY', last);
console.log('KEEP', input === null ? 0 : 1, last === null ? 0 : 1);
