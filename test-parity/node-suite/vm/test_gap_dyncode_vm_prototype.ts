import vm from 'node:vm';
// DEC 250 locks Array.prototype; the vm source patches a user array
// prototype UAP that the array adopts, a write the compiler cannot see either.
const UAP: any = Object.create(Array.prototype);
(globalThis as any).UAP = UAP;
const a: number[] = Object.setPrototypeOf([1, 2], UAP);
vm.runInThisContext("UAP.join = function () { return 'vm-patch'; }");
console.log(a.join(","));
