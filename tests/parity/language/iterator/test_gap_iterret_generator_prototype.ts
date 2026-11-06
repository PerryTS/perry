// Retained Node probe for the existing own-method generator representation.
// DEC 250: %GeneratorPrototype% is locked in a compiled program, so the patch is
// refused and return() runs the builtin; tests/parity/expected holds Perry output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
let calls=0;let closes=0;
function* gen(){try{yield 1;yield 2;}finally{closes++;}}
const gp:any=Object.getPrototypeOf(Object.getPrototypeOf(gen()));
const savedReturn=gp.return;
locked(()=>{gp.return=function(value:any){calls++;return savedReturn.call(this,value);};});
calls=0;closes=0;const g=gen();g.next();g.return(undefined);console.log('patched-generator-return',calls,closes);
locked(()=>{gp.return=savedReturn;});
