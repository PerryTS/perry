// Supplemental lifetime/latency probe. Keep worker.ts and the fixed 38-row
// baseline unchanged. Link this same application object against both runtimes.
import { readFileSync } from 'node:fs';
declare function gc(): void;

const source = readFileSync(process.argv[2], 'utf8');
const mode = process.argv[3];
const iterations = Number(process.argv[4]);
const operation = process.argv[5];
let retained: any[] = [];
let last: any = null;
let input: any = null;
if (operation === 'stringify') input = JSON.parse('{"data":' + source + '}').data;
let parseMs = 0;
let stringifyMs = 0;
let parseMaxMs = 0;
let stringifyMaxMs = 0;

// The discard result dies with this frame. The caller receives only a scalar,
// unlike the baseline worker, which keeps its newest output in `last`.
function one(): number {
    if (operation === 'stringify') {
        const started = performance.now();
        const text = JSON.stringify(input);
        const elapsed = performance.now() - started;
        stringifyMs += elapsed;
        if (elapsed > stringifyMaxMs) stringifyMaxMs = elapsed;
        if (mode === 'retain') retained.push(text);
        if (mode === 'latest') last = text;
        return text.length;
    }
    const started = performance.now();
    const value: any = JSON.parse(source);
    const elapsed = performance.now() - started;
    parseMs += elapsed;
    if (elapsed > parseMaxMs) parseMaxMs = elapsed;
    if (operation === 'roundtrip') {
        const stringifyStarted = performance.now();
        const text = JSON.stringify(value);
        const stringifyElapsed = performance.now() - stringifyStarted;
        stringifyMs += stringifyElapsed;
        if (stringifyElapsed > stringifyMaxMs) stringifyMaxMs = stringifyElapsed;
        return text.length;
    }
    if (mode === 'retain') retained.push(value);
    if (mode === 'latest') last = value;
    return value === null ? 0 : 1;
}

for (let i = 0; i < 3; i++) one();
retained = [];
last = null;
gc();
parseMs = 0;
stringifyMs = 0;
parseMaxMs = 0;
stringifyMaxMs = 0;
const rssBefore = process.memoryUsage().rss;
const cpuBefore = process.cpuUsage();
const started = performance.now();
let checksum = 0;
for (let i = 0; i < iterations; i++) checksum += one();
const loopMs = performance.now() - started;
const cpuLoop = process.cpuUsage();
const rssLoop = process.memoryUsage().rss;

// Charge an explicit cleanup after the loop. A faster API return is not proof
// of lower total CPU if it only leaves more collection debt behind.
const drainCpuBefore = process.cpuUsage();
const drainStarted = performance.now();
gc();
const drainMs = performance.now() - drainStarted;
const cpuDrained = process.cpuUsage();
const rssDrained = process.memoryUsage().rss;
const loopCpuUs = cpuLoop.user + cpuLoop.system - cpuBefore.user - cpuBefore.system;
const drainCpuUs = cpuDrained.user + cpuDrained.system - drainCpuBefore.user - drainCpuBefore.system;
console.log('LIFETIME', JSON.stringify({
    mode, operation, iterations, checksum, parseMs, stringifyMs, parseMaxMs,
    stringifyMaxMs, loopMs, drainMs, loopCpuUs, drainCpuUs,
    totalCpuUs: loopCpuUs + drainCpuUs, rssBefore, rssLoop, rssDrained,
    retained: retained.length,
}));
// Read retained output after cleanup so the drain must preserve real roots.
if (mode === 'retain') console.log('VERIFY', JSON.stringify(retained[retained.length - 1]));
if (mode === 'latest') console.log('VERIFY', JSON.stringify(last));
