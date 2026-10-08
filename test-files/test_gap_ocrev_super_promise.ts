class P extends Promise{then(a,b){return super.then(a,b)}} async function run(){const p=new P(r=>r(3)); console.log(await p.then(v=>v+1)); console.log(await p)} run();
