function F(){}
const G:any=F.bind(null);
const p={};
let n=0;
Object.defineProperty(G,"prototype",{get(){n++;return p}});
class C extends G{}
console.log(n,Object.getPrototypeOf(C.prototype)===p,n);
