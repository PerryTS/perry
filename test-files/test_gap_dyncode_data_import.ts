const specifier = process.argv[2] || "data:text/javascript,export const n=(Array.prototype.join=function(){return 'data-patch'},42)";
const result = await import(specifier);
console.log(result.n);
console.log([1, 2].join(','));
