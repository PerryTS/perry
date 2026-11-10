// The record mode/done/cursor are primitives; yielded values can be pointers.
let original = "";
for (let i = 0; i < 64; i++) original += "x";
const roots: any = [{name: original}, {name: original}, original];
const values = Array.prototype[Symbol.iterator];
function exercise(source: any): number {
    let checksum = 0;
    for (const value of source) {
        const keep: boolean = value as any;
        const garbage: any = [];
        for (let i = 0; i < 350000; i++) garbage.push({i});
        if (keep) checksum += 13;
        if (garbage.length > 0) checksum++;
    }
    return checksum;
}
console.log(exercise(roots));
roots[Symbol.iterator] = function () { return values.call(this); };
console.log(exercise(roots));
const [first, second, text] = roots;
let alias: any = text;
alias += "!";
console.log(roots[2].length, alias.length, first.name.length, second.name.length);
