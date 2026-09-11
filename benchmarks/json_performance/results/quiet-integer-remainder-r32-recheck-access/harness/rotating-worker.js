// Generated from rotating-worker.ts by erasing its type annotations.
// Setup and input allocation stay outside timing. The eight preloaded strings
// differ in one value, retaining the same byte size and property order.
import { readFileSync } from 'node:fs';

const prefix = process.argv[2];
const operation = process.argv[3];
const iterations = Number(process.argv[4]);
const warmup = Number(process.argv[5]);
const verify = process.argv[6] === 'verify';
const mode = process.argv[7] || 'rotating';
const sources = [];
for (let i = 0; i < 8; i++) {
    sources.push(readFileSync(prefix + '.' + String(i) + '.json', 'utf8'));
}
let last = null;
let cursor = 0;
let checksum = 0;

function run(count) {
    let sum = 0;
    if (operation === 'select') {
        for (let i = 0; i < count; i++) {
            const text = sources[cursor];
            cursor = (cursor + 1) & 7;
            sum += text.length;
        }
    } else if (mode === 'same') {
        for (let i = 0; i < count; i++) {
            const value = JSON.parse(sources[0]);
            last = value;
            sum += value === null ? 0 : 1;
        }
    } else {
        for (let i = 0; i < count; i++) {
            const value = JSON.parse(sources[cursor]);
            cursor = (cursor + 1) & 7;
            last = value;
            sum += value === null ? 0 : 1;
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
console.log('RESULT', elapsedMs, cpuAfter.user - cpuBefore.user,
    cpuAfter.system - cpuBefore.system, rssBefore, rssAfter, checksum, 0);
if (verify) {
    // Reparse every member outside measurement: checking only the last member
    // cannot detect a stale cached value returned for a different input.
    for (let i = 0; i < 8; i++) {
        console.log('VERIFY', i, JSON.stringify(JSON.parse(sources[i])));
    }
    console.log('LAST', JSON.stringify(last));
}
console.log('KEEP', sources.length, last === null ? 0 : 1);
