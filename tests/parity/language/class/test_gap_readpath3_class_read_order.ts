// The declared CLASS read route must retain ordinary Get semantics while
// weak intrinsic CLASS receivers use their branded pre-fallback route.
class ReaderBase {
    _parseSync(n: number) { return (this as any)._parse(n); }
}
class Reader extends ReaderBase {
    offset = 7;
    _parse(n: number) { return n + this.offset; }
}
function parse(reader: any, n: number) { return reader._parseSync(n); }
const readers: any[] = [new Reader(), {}];
const reader: any = readers[0];
for (let i = 0; i < 100; i++) parse(reader, i);
console.log(parse(reader, 3), reader._parse === Reader.prototype._parse);
const symbol = Symbol("_parseSync");
reader[symbol] = function() { return 63; };
console.log("key kinds", parse(reader, 3), reader[symbol]());
delete reader[symbol];
for (let i = 0; i < 100; i++) void reader.late;
console.log("absent", typeof reader.late);
(Reader.prototype as any).late = 71;
console.log("present", reader.late);
delete (Reader.prototype as any).late;
console.log("absent again", typeof reader.late);
reader._parse = function(n: number) { return n + 20; };
console.log(parse(reader, 3));
delete reader._parse;
console.log(parse(reader, 3));
const proto: any = Reader.prototype;
let reads = 0;
Object.defineProperty(proto, "_parseSync", { configurable: true, get() {
    reads++;
    console.log("receiver", this === reader);
    return function(n: number) { return n + this.offset + 30; };
}});
console.log(parse(reader, 3), reads);
Object.defineProperty(proto, "_parseSync", { configurable: true, get() {
    reads++;
    return undefined;
}});
console.log(typeof reader._parseSync, reads);
delete proto._parseSync;
console.log(parse(reader, 3));
Object.setPrototypeOf(reader, new Proxy(proto, { get(target, key, receiver) {
    if (key === "_parseSync") return function(n: number) { return n + 50; };
    return Reflect.get(target, key, receiver);
}}));
console.log(parse(reader, 3));
Object.setPrototypeOf(reader, null);
console.log(typeof reader._parseSync);
Object.setPrototypeOf(reader, proto);
console.log(parse(reader, 3));
