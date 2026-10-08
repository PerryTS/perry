const itp: any = Object.getPrototypeOf([1][Symbol.iterator]());
let receiver: any;
const [a = (itp.return = function(this: any) { receiver = this; return {done:true}; }, 42)] = [undefined, 15, 16];
delete itp.return;
console.log("late-close", a, receiver.next().value, receiver.next().value, receiver.next().done);
const trace: string[] = [];
const [first, second = (trace.push("default"), 7), ...rest] = [1, undefined, 3, 4];
console.log("rest", first, second, rest.join(","), trace.join(","));
