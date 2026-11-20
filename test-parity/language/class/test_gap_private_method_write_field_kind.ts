class C {
 n:any=1;
 constructor(){this.#change();}
 #change(){this.n="oops";}
}
const c=new C();
console.log(c.n+1);
