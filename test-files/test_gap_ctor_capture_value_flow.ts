class Payload { value: number; constructor(value: number) { this.value = value; } }
class Box { item: Payload; constructor(item: Payload) { this.item = item; } }
class Alternate { item: Payload; constructor(item: Payload) { this.item = new Payload(item.value + 1); } }
function maker(Ctor: any, other: any) {
  return class Maker {
    klass = Ctor;
    make(item: Payload) { return new this.klass(item); }
    captured(item: Payload) { return new Ctor(item); }
    other() { return other; }
  };
}
const First = maker(Box, { irrelevant: 99 });
const Second = maker(Alternate, function unused() { return 'other'; });
const a = new First();
const b = new Second();
console.log(a.make(new Payload(7)).item.value, b.captured(new Payload(8)).item.value, a.other().irrelevant);
function Replacing(this: any, item: Payload) { this.dead = true; return new Box(item); }
const Holder = { klass: Replacing };
const Bound = Holder.klass.bind(null, new Payload(11));
console.log(new (Bound as any)().item.value);
const constructors: any[] = [Box, Alternate];
console.log(constructors.map((C) => new C(new Payload(20)).item.value).join(','));
const DateCtor: any = globalThis.Date;
const RegExpCtor: any = globalThis.RegExp;
const MapCtor: any = globalThis.Map;
const SetCtor: any = globalThis.Set;
const ArrayCtor: any = globalThis.Array;
const TypedCtor: any = globalThis.Uint8Array;
const PromiseCtor: any = globalThis.Promise;
const ErrorCtor: any = globalThis.TypeError;
const AggregateCtor: any = globalThis.AggregateError;
console.log(new DateCtor(0).getUTCFullYear(), new RegExpCtor('a+', 'i').test('AA'));
console.log(new MapCtor([['x', 1]]).get('x'), new SetCtor([1, 1, 2]).size, new ArrayCtor(1, 2).join(','), new TypedCtor([3, 4])[1]);
const cause = new Payload(33);
console.log(new ErrorCtor('message', { cause }).cause.value);
const error = new AggregateCtor([cause], 'aggregate', { cause });
console.log(error.errors[0].value, error.cause.value);
new PromiseCtor(...[(resolve: any) => resolve(new Payload(44))]).then((p: Payload) => console.log(p.value));
