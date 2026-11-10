// Symbol data and accessors must remain ordinary properties across shape edits.
const key = Symbol('value');
function read(object: any, symbol: symbol): any { return object[symbol]; }
function nested(object: any, symbol: symbol): any { return object[symbol].value; }
const object: any = { prefix: 1, [key]: { value: 7 } };
console.log('warm', nested(object, key), nested(object, key), read(object, key) === object[key]);
delete object.prefix;
console.log('shifted', nested(object, key), nested(object, key), Object.keys(object).length);
object[key] = { value: 9 };
console.log('stored', nested(object, key), nested(object, key));
let calls = 0;
Object.defineProperty(object, key, { get() { calls++; return { value: 20 + calls }; }, configurable: true, enumerable: true });
console.log('accessor', nested(object, key), nested(object, key), calls);
Object.defineProperty(object, key, { value: 42, writable: false, configurable: true, enumerable: false });
console.log('descriptor', read(object, key), Reflect.set(object, key, 99), read(object, key));
console.log('reflection', Object.getOwnPropertySymbols(object).map(String).join(','), Reflect.ownKeys(object).map(String).join(','), JSON.stringify(object));
console.log('spread', Object.getOwnPropertySymbols({ ...object }).length);
console.log('delete', Reflect.deleteProperty(object, key), Object.getOwnPropertySymbols(object).length, read(object, key));
object[key] = 13;
console.log('recreated', read(object, key), Object.getOwnPropertyDescriptor(object, key)!.enumerable);
const frozen = Object.freeze({ [key]: 4 });
console.log('frozen', Reflect.set(frozen, key, 8), frozen[key], Object.isFrozen(frozen));
// Repeated loop reads exercise one IC site rather than separately inlined calls.
const warmKey = Symbol('warm');
const warmObject: any = { prefix: 1, [warmKey]: 17 };
let warmTotal = 0;
for (let i = 0; i < 20; i++) warmTotal += warmObject[warmKey];
console.log('loop-cache', warmTotal);
