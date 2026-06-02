// process.nextTick(cb) returns undefined.
console.log("returns undefined:", process.nextTick(() => {}) === undefined);
