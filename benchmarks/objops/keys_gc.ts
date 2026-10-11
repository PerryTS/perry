function copy(v: any) { return Object.keys(v); }
let sum = 0;
for (let i = 0; i < 1000; i++) {
    const key = 'first-long-key-' + i;
    const v: any = { [key]: i, other_long_key: i, final_long_key: i };
    const result = copy(v);
    sum += result.length + result[0].length + result[1].length + result[2].length;
    result.reverse();
    sum += result[2].length;
}
console.log(sum);
