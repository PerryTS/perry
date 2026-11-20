// Objects and arrays
const obj = { name: "alice", age: 30 };
console.log(obj.name);
console.log(obj.age);
obj.age = 31;
console.log(obj.age);

const arr = [10, 20, 30];
console.log(arr.length);
console.log(arr[0]);
arr.push(40);
console.log(arr.length);
console.log(arr.join("-"));

const keys = Object.keys(obj);
console.log(keys.length);
