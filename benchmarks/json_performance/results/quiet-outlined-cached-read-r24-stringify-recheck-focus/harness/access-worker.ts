import { readFileSync } from 'node:fs';
const text = readFileSync(process.argv[2], 'utf8');
const mode = process.argv[3];
const iterations = Number(process.argv[4]);
const warmup = Number(process.argv[5]);
const records: any = JSON.parse(text);
const length = records.length;
function run(rows: any, count: number): number {
    let sum = 0;
    let cursor = 0;
    if (mode === 'repeat') {
        for (let i = 0; i < count; i++) sum += rows[7].id;
    } else if (mode === 'random') {
        for (let i = 0; i < count; i++) {
            cursor = (cursor * 17 + 7) % length;
            const index = cursor;
            sum += rows[index].id;
        }
    } else if (mode === 'fields') {
        for (let i = 0; i < count; i++) {
            const index = i % length;
            sum += rows[index].id;
            sum += rows[index].name.length;
            sum += rows[index].active ? 1 : 0;
        }
    } else {
        for (let i = 0; i < count; i++) {
            const index = i % length;
            sum += rows[index].id;
        }
    }
    return sum;
}
let checksum = run(records, warmup);
const rssBefore = process.memoryUsage().rss;
const cpuBefore = process.cpuUsage();
const started = performance.now();
checksum += run(records, iterations);
const elapsed = performance.now() - started;
const cpuAfter = process.cpuUsage();
console.log('RESULT', elapsed, cpuAfter.user - cpuBefore.user,
    cpuAfter.system - cpuBefore.system, rssBefore, process.memoryUsage().rss, checksum, 0);
console.log('KEEP', records.length);
