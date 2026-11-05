const specifier = process.argv[2] || "data:text/javascript,export const n=(Array.prototype.join=function(){return 'data-patch'},42)";
let result: any;
try { result = await import(specifier); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
if (result) console.log(result.n);
console.log([1, 2].join(','));
