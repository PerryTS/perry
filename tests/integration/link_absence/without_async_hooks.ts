// Promise and timer paths cannot observe hooks that no program API reaches.
const values: number[] = [];
Promise.resolve(3).then(value => { values.push(value); console.log(values.join(",")); });
setTimeout(() => console.log("timer"), 0);
