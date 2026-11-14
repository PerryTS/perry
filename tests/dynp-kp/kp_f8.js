const H=[Object]; const orig=Object.prototype.hasOwnProperty;
const rd=()=>H[0].prototype.hasOwnProperty;
console.log(rd()===orig);
Object.prototype.hasOwnProperty=function(){return "mine"}; console.log(typeof rd(), ({}).hasOwnProperty("x"));
delete Object.prototype.hasOwnProperty; console.log(typeof rd(), "hasOwnProperty" in {});
Object.defineProperty(Object.prototype,"hasOwnProperty",{get(){return ()=>"got"},configurable:true}); console.log(rd()());
Object.defineProperty(Object.prototype,"hasOwnProperty",{value:orig,writable:true,configurable:true,enumerable:false}); console.log(rd()===orig, ({a:1}).hasOwnProperty("a"), Object.prototype.hasOwnProperty.call({b:1},"b"));
