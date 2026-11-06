// #11915: prototype mutation and actual method selection.
const a = [1, 2];
const b: any = [3, 4];
b.push = function (v: number) { return 100 + v; };
console.log(a.push(5), a.indexOf(2), a.map(x => x * 2).join(','));
console.log(b.push(5));
console.log(a.slice(1).join(','));
console.log('abcd'.slice(1, 3));
console.log(({x: 1}).hasOwnProperty('x'));
