import { readFileSync } from 'node:fs';
const text = readFileSync(process.argv[2], 'utf8');
const trips = Number(process.argv[3]);
const iterations = Number(process.argv[4]);
const warmup = Number(process.argv[5]);
const records: any = JSON.parse(text);
function sum(rows: any, count: number): number {
    let value = 0;
    for (let i = 0; i < count; i++) value += rows[7].id;
    return value;
}
function run(rows: any, repeats: number, count: number): number {
    let checksum = 0;
    for (let i = 0; i < repeats; i++) checksum += sum(rows, count);
    return checksum;
}
let checksum = run(records, warmup, trips);
const rssBefore = process.memoryUsage().rss;
const cpuBefore = process.cpuUsage();
const started = performance.now();
checksum += run(records, iterations, trips);
const elapsed = performance.now() - started;
const cpuAfter = process.cpuUsage();
console.log('RESULT', elapsed, cpuAfter.user - cpuBefore.user,
    cpuAfter.system - cpuBefore.system, rssBefore, process.memoryUsage().rss, checksum, 0);
console.log('KEEP', records.length);
