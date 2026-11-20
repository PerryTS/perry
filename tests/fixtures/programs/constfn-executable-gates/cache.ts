import { make, other, seen } from './producer.ts';
function invoke(receiver: any) { return receiver.m(); }
const first = make(17), second = make(29), different = other(5);
let sum = 0;
for (let i = 0; i < 4096; i++) sum += invoke(i % 2 ? second : first);
console.log(seen, sum, invoke(different), first.m !== second.m);
