const boundRandom = Math.random.bind(Math);
console.log(typeof boundRandom);
console.log(typeof boundRandom());

let threwTypeError = false;
try { Function.prototype.bind.call({}); } catch (error) { threwTypeError = error instanceof TypeError; }
console.log(threwTypeError);
