import { parallelMap } from 'perry/thread';

// Two launches call the same concat site. Every cached slot must belong to
// the executing worker, even after the first launch retires its arenas.
function checkConcat(n: number) {
    let length = 0;
    for (let i = 0; i < 128; i++) {
        const text = 'worker-' + i;
        if (text !== 'worker-'.concat(String(i))) {
            throw new Error('worker concat changed: ' + text);
        }
        length = text.length;
    }
    return length + n;
}
const first = parallelMap([1, 2], (n: number) => checkConcat(n));
const second = parallelMap([1, 2], (n: number) => checkConcat(n));
console.log(first[0], first[1], second[0], second[1]);
