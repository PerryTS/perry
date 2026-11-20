function identity(fn: any) { return fn; }
function make() {
 const read = () => later(7) as number;
 const later = identity((x: number) => x + 1);
 return read;
}
console.log(make()());
