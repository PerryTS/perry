const events: string[]=[];
const bad=Promise.resolve(0);
Object.defineProperty(bad,"constructor",{get(){throw "ctor";}});
async function* rows() { await 0; yield 1; }
async function main() {
 const it=rows(); const first=it.next().then(r=>events.push("first:"+r.value));
 const pending: any[]=[]; let rejected=0;
 for(let i=0;i<8192;i++) pending.push(it.return(bad).then(()=>events.push("unexpected"),e=>{if(e==="ctor") rejected++;}));
 queueMicrotask(()=>events.push("micro"));
 await Promise.all(pending); await first;
 events.push("rejected:"+rejected); console.log(events.join("|"));
}
main();
