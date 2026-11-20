let events="";
function make():any{
 let n=0;
 return {[Symbol.iterator](){return this;},next(){
  const value=++n;
  return new Proxy({done:value>2,value},{get(target,key,receiver){
   events+=String(key)+"|";return Reflect.get(target,key,receiver);
  }});
 },return(){events+="return|";return {done:true};}};
}
let text="";for(const x of make())text+=x;
console.log("proxy-result-walk",text,events);
events="";const [,x]=make();
console.log("proxy-result-hole",x,events);
events="";let y:any;[,y]=make();
console.log("proxy-result-assignment",y,events);
