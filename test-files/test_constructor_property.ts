const d = new Date(2024, 0, 15);
console.log(typeof d.constructor);
console.log(d.constructor === Date);
const cloned = new (d.constructor as any)(d.getTime());
console.log(cloned.getTime() === d.getTime());

const arr = [1, 2, 3];
console.log(typeof arr.constructor);
console.log(arr.constructor === Array);

const obj = { x: 1 };
console.log(typeof obj.constructor);
console.log(obj.constructor === Object);
