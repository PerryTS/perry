// Retained Node probe for the existing own-method generator representation.
// Both unmodified main and this branch hide a patched GeneratorPrototype.
let calls=0;let closes=0;
function* gen(){try{yield 1;yield 2;}finally{closes++;}}
const gp:any=Object.getPrototypeOf(Object.getPrototypeOf(gen()));
const savedReturn=gp.return;
gp.return=function(value:any){calls++;return savedReturn.call(this,value);};
calls=0;closes=0;const g=gen();g.next();g.return(undefined);console.log('patched-generator-return',calls,closes);
gp.return=savedReturn;
