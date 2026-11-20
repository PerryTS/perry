function apply(target: any, properties: any) {
  return Object.defineProperties(target, properties);
}

const target: any = {};
const bag: any = {};
bag.a = { value: 1, enumerable: true };
Object.defineProperty(bag, 'b', {
  enumerable: true,
  get() {
    console.log('decode observes a', Object.prototype.hasOwnProperty.call(target, 'a'));
    return { value: 2, enumerable: true };
  }
});
apply(target, bag);
console.log('after successful decode', target.a, target.b);

const rejected: any = {};
const invalid: any = {};
invalid.a = { value: 3 };
invalid.b = 4;
try {
  apply(rejected, invalid);
} catch (error) {
  console.log('decode failed', error instanceof TypeError);
}
console.log('after rejected decode', Object.prototype.hasOwnProperty.call(rejected, 'a'));

const events: string[] = [];
const source: any = {};
source.a = { value: 5 };
source.b = { value: 6 };
const observed = new Proxy(source, {
  ownKeys(object) {
    events.push('keys');
    return Reflect.ownKeys(object);
  },
  getOwnPropertyDescriptor(object, key) {
    events.push('own:' + String(key));
    return Reflect.getOwnPropertyDescriptor(object, key);
  },
  get(object, key, receiver) {
    events.push('get:' + String(key));
    return Reflect.get(object, key, receiver);
  }
});
apply({}, observed);
console.log('proxy order', events.join(','));
