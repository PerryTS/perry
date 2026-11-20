async function run() {
  for (const make of [
    () => [{n:1},{n:2},{n:3}].values(),
    () => new Map([[1,{n:1}],[2,{n:2}],[3,{n:3}]]).values(),
    () => new Set([{n:1},{n:2},{n:3}]).values(),
  ]) {
    const it:any=make();
    let text="";
    for (const x of it) {
      await Promise.resolve(0);
      const garbage:any[]=[];
      for(let i=0;i<30000;i++)garbage.push({n:i});
      text+=x.n;
      if(x.n===2)continue;
    }
    console.log("awaited-native",text);
    const [head,...tail]:any=make();
    await Promise.resolve(0);
    console.log("awaited-rest",head.n,tail[0].n,tail[1].n);
  }
  const it:any=[{n:4},{n:5}].values();let closes=0;
  it.return=function(){closes++;return {done:true};};
  for(const x of it){await Promise.resolve(0);console.log("awaited-break",x.n);break;}
  console.log("awaited-close",closes);
}
run().catch((e)=>{console.log("ERROR",String(e));});
