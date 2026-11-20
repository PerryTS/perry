function read(o: any, k: any): any { return o[k]; }
for (const holder of [Buffer.from([21, 22]), new Uint8Array([31, 32]), new Int16Array([41, 42])]) {
 const middle: any = Object.create(holder);
 const child: any = Object.create(middle);
 console.log("indices", read(child, 0), read(child, "1"), read(child, "2"));
 child[0] = 53;
 console.log("shadow", read(child, "0"), read(child, 1));
 console.log("invalid", read(child, "-0"), read(child, "-1"), read(child, "NaN"), read(child, "1.5"));
}
const bytes: any = Buffer.from([21, 22]);
Object.defineProperty(bytes, "01", {value: 61, configurable: true});
console.log("ordinary", read(Object.create(bytes), "01"));
