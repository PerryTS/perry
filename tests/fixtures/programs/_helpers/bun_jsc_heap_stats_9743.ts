// Compiled with --platform bun by issue_9743_bun_jsc_heap_stats.rs.
// Its preamble supplies heapStats through one of the supported import forms.
import { gc } from 'bun';

function check(condition: boolean, message: string) {
    if (!condition) throw new Error(message);
}

const keys = [
    'extraMemorySize', 'globalObjectCount', 'heapCapacity', 'heapSize', 'mimalloc',
    'objectCount', 'objectTypeCounts', 'protectedGlobalObjectCount',
    'protectedObjectCount', 'protectedObjectTypeCounts',
].sort().join(',');

function checkStats(stats: any) {
    check(Object.keys(stats).sort().join(',') === keys, 'heapStats keys');
    for (const key of ['heapSize', 'heapCapacity', 'extraMemorySize', 'objectCount',
        'protectedObjectCount', 'globalObjectCount', 'protectedGlobalObjectCount']) {
        check(typeof stats[key] === 'number' && Number.isFinite(stats[key]) && stats[key] >= 0,
            'invalid scalar ' + key);
    }
    for (const key of ['objectTypeCounts', 'protectedObjectTypeCounts', 'mimalloc']) {
        const counts = stats[key];
        check(typeof counts === 'object' && counts !== null, 'invalid counters ' + key);
        for (const type of Object.keys(counts)) {
            check(typeof counts[type] === 'number' && Number.isFinite(counts[type]) && counts[type] >= 0,
                'invalid counter ' + key + '.' + type);
        }
    }
    check(Object.keys(stats.objectTypeCounts).length > 0, 'missing heap census');
    check(Object.keys(stats.mimalloc).length > 0, 'missing allocator counters');
    check(stats.heapCapacity >= stats.heapSize, 'capacity smaller than usage');
    check(stats.protectedObjectCount <= stats.objectCount, 'protected count exceeds population');
}

checkStats(heapStats());
checkStats(heapStats(true));
gc(true);
const before = heapStats();
const retained: any[] = [];
for (let i = 0; i < 256; i++) retained.push({ value: i, text: 'cell-' + i });
gc(true);
const after = heapStats(true);
checkStats(before);
checkStats(after);
check(after.objectCount > before.objectCount, 'retained objects must increase the census');
check(after.heapSize > before.heapSize, 'retained objects must increase heap usage');
console.log('heapStats shapes ok');
console.log('retained', retained.length, retained[0].value, retained[255].value);
console.log('heapStats growth ok');
