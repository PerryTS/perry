function scan(a: any): string { let out = ""; for (const x of a) out += x; return out; }
const values = Array.prototype[Symbol.iterator];
const own: any = [1,2,3];
own[Symbol.iterator] = values.bind([7,8]);
console.log("bound-own", scan(own));
const family: any = Object.getPrototypeOf([].values());
const next = family.next;
const other: any = values.call([9,10]);
try { family.next = next.bind(other); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("bound-next", scan([1,2,3]));
try { family.next = next; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const method = function() { return values.call([4,5]); };
try { delete (Array.prototype as any)[Symbol.iterator]; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
try { (Object.prototype as any)[Symbol.iterator] = method; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("inherited-after-delete", scan([1,2,3]));
delete (Object.prototype as any)[Symbol.iterator];
try { scan([1,2,3]); console.log("delete-missed"); }
catch (e) { console.log("delete-throws", e instanceof TypeError); }
try { Array.prototype[Symbol.iterator] = values; } catch (e) { console.log("DEC 250: " + (e as Error).message); }

