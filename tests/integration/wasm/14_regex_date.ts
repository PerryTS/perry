// RegExp and Date
const re = new RegExp("^hello", "i");
console.log(re.test("Hello World"));
console.log(re.test("goodbye"));

const now = Date.now();
console.log(now > 0);

const d = new Date(1700000000000);
console.log(d.getFullYear());
