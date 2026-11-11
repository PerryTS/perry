// Literal equality must accept both Number encodings without coercing tags.
function equals80(value: any) { return value === 80; }
function differs80(value: any) { return 80 !== value; }
const values: any[] = [80, Number("80"), parseInt("80"), JSON.parse("80"),
  "P".charCodeAt(0), 79, 80.5, -80, 0, -0, NaN, Infinity, -Infinity,
  undefined, null, true, false, "80", 80n, {}, new Number(80)];
for (const value of values) console.log(equals80(value), differs80(value));

function zero(value: any) { return value === 0; }
function negative(value: any) { return value === -2147483648; }
for (const value of [0, -0, JSON.parse("0"), -2147483648,
  JSON.parse("-2147483648"), 2147483648, NaN, null]) {
  console.log(zero(value), negative(value));
}

// A Boolean produced by a comparison remains a Boolean through stores,
// captures, calls and a following operation.
function pair(value: any) {
  const equal = equals80(value);
  const unequal = differs80(value);
  const out: any = { equal, unequal };
  const capture = () => equal;
  return [out.equal === true, out.unequal === false,
    !capture(), typeof out.equal, Number(out.equal) + Number(out.unequal)];
}
console.log(JSON.stringify(pair(80)), JSON.stringify(pair("80")));

function scalarStore(receiver: any, value: any) {
  receiver.number = value | 0;
  receiver.boolean = !!value;
  return [receiver.number, receiver.boolean];
}
const receiver: any = { number: 0, boolean: false };
for (const value of ["80", NaN, -2147483648, { valueOf() { return 80; } }]) {
  console.log(JSON.stringify(scalarStore(receiver, value)));
}
for (let i = 0; i < 5; i++) {
  console.log(JSON.stringify(scalarStore({}, i)));
}
for (const value of [80n, Object(80n), Symbol("number")]) {
  try {
    scalarStore(receiver, value);
    console.log("unexpected scalar");
  } catch (error: any) {
    console.log(error.name);
  }
}
