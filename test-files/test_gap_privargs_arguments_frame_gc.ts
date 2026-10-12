"use strict";
function observe(a: any, collect: boolean) {
    if (collect) {
        let junk: any[] = [];
        for (let i = 0; i < 5000; i++) junk.push({ n: i, text: 'churn-' + i });
        if (junk.length !== 5000) throw new Error('churn');
    }
    const first = arguments;
    return first[0].n + (first === arguments ? 1 : 1000);
}
function relay(a: number) { return observe(a, true); }
let total = 0;
for (let i = 0; i < 12; i++) total += relay({ n: i } as any);
console.log('frame', total);
function captureAfterChurn(a: any) {
    for (let i = 0; i < 1000; i++) observe({ n: i }, false);
    return () => arguments[0];
}
const read = captureAfterChurn({ n: 41 });
console.log('retained', read().n);
