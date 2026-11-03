class Ordinary {value:number;constructor(value:number){this.value=value}}
const value=new Ordinary(42);console.log(value.value,value instanceof Ordinary);
Promise.all([Promise.resolve(7),9]).then(values=>console.log(values.join(",")));
