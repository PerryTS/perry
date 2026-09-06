// Identical source for Perry, Node and Bun. Inputs are produced outside timing.
import { readFileSync } from 'node:fs';

const file = process.argv[2];
const operation = process.argv[3];
const iterations = Number(process.argv[4]);
const warmup = Number(process.argv[5]);
const source = readFileSync(file, 'utf8');
let input: any = null;
// An object envelope forces eager Perry parsing of nested arrays. This prevents
// stringify-only from measuring the lazy original-text copy shortcut.
if (operation === 'stringify' || operation === 'pretty' || operation === 'retain-stringify') {
    input = JSON.parse('{"data":' + source + '}').data;
}
let retained: any[] = [];
let last: any = null;
let checksum = 0;
function run(count: number): number {
    let sum = 0;
    if (operation === 'parse' || operation === 'retain-parse') {
        for (let i = 0; i < count; i++) {
            const value: any = JSON.parse(source);
            last = value;
            if (operation === 'retain-parse') retained.push(value);
            sum += value === null ? 0 : 1;
        }
    } else if (operation === 'scan' || operation === 'sparse') {
        for (let i = 0; i < count; i++) {
            const value: any = JSON.parse(source);
            if (operation === 'scan') {
                for (let j = 0; j < value.length; j++) sum += value[j].id;
            } else {
                sum += value[0].id + value[value.length - 1].id;
            }
            last = value;
        }
    } else if (operation === 'roundtrip') {
        for (let i = 0; i < count; i++) {
            const value: any = JSON.parse(source);
            const result = JSON.stringify(value);
            sum += result.length;
            last = result;
        }
    } else if (operation === 'stringify' || operation === 'retain-stringify') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input);
            sum += result.length;
            last = result;
            if (operation === 'retain-stringify') retained.push(result);
        }
    } else if (operation === 'pretty') {
        for (let i = 0; i < count; i++) {
            const result = JSON.stringify(input, null, 2);
            sum += result.length;
            last = result;
        }
    }
    return sum;
}
checksum += run(warmup);
const rssBefore = process.memoryUsage().rss;
const cpuBefore = process.cpuUsage();
const started = performance.now();
checksum += run(iterations);
const elapsedMs = performance.now() - started;
const cpuAfter = process.cpuUsage();
const rssAfter = process.memoryUsage().rss;
const userUs = cpuAfter.user - cpuBefore.user;
const systemUs = cpuAfter.system - cpuBefore.system;
console.log('RESULT', elapsedMs, userUs, systemUs, rssBefore, rssAfter, checksum, retained.length);
// Check final parse/stringify output outside timing. The controller hashes this
// against Node; touching every byte also makes produced output observable.
if (process.argv[6] === 'verify') {
    if (operation === 'parse' || operation === 'scan' || operation === 'sparse' || operation === 'retain-parse') {
        console.log('VERIFY', JSON.stringify(last));
    } else {
        console.log('VERIFY', last);
    }
}
// Keep retained graphs live through measurement and verification.
console.log('KEEP', retained.length, last === null ? 0 : 1);
