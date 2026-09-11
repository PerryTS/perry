// Post-parse index arithmetic uses dynamic numeric remainder.
function remainder(a: any, b: any): any { return a % b; }
function describe(v: any): string {
    if (typeof v === 'number') {
        if (Object.is(v, -0)) return '-0';
        if (Number.isNaN(v)) return 'NaN';
    }
    return typeof v + ':' + String(v);
}
const values: number[] = [-Infinity, -4294967296, -1.5, -1, -0, 0, 0.5, 1, 17, 65537, 2147483648, 4294967295, 4294967295.5, 4294967296, 9007199254740991, Infinity, NaN];
for (let i = 0; i < values.length; i++) {
    for (let j = 0; j < values.length; j++) console.log(i, j, describe(remainder(values[i], values[j])));
}
const boxed: any[] = ['4294967295', true, false, null, undefined, 43n, -43n];
for (let i = 0; i < boxed.length; i++) {
    try { console.log('number', i, describe(remainder(boxed[i], 7))); }
    catch (e) { console.log('number', i, e instanceof TypeError ? 'TypeError' : 'unexpected'); }
    try { console.log('bigint', i, describe(remainder(boxed[i], 7n))); }
    catch (e) { console.log('bigint', i, e instanceof TypeError ? 'TypeError' : 'unexpected'); }
}
