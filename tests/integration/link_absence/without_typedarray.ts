const values=[3,1,2];console.log(values.toSorted().join(","));
console.log(Array.from("abc").join(","));
const ordinary:any={0:7};console.log(ordinary[0],"hello".slice(1));
// Array sort uses the common byte store for private rooted workspaces even
// when the program never names a typed-array API. Exceed its stack workspace.
const large:number[]=[];
for(let i=0;i<129;i++)large.push(129-i);
large.sort((a,b)=>a-b);
console.log(large.length,large[0],large[128]);
