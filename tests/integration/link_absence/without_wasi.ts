class Ordinary {value:number;constructor(value:number){this.value=value}}
const value=new Ordinary(42);
console.log(value.value,value instanceof Ordinary,Object.keys(value).join(","));
console.log(Object.getOwnPropertyNames({value:7}).join(","));
