// The program's first inherited read of a function is %Function.prototype%'s
// member, before anything else has built the realm global.
const f: any = function f() {};
const t = f.toString;
console.log("1", typeof t, t === Function.prototype.toString);
const g: any = (x: number) => x;
console.log("2", typeof g.hasOwnProperty, g.constructor === Function);
