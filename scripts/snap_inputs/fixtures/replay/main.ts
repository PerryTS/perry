// Counterexample component model: deliberately seed the heap at an unsafe
// child boundary. This does not implement or exercise an image loader.
const wrong=process.env.SNAP21_UNSAFE_REPLAY;
let childDone=wrong!==undefined;
let childRuns=childDone?1:0;
const output:string[]=wrong==="R1"?["before"]:[];
let durableCell=wrong==="R2"?1:0;
function child(){if(childDone)return;childDone=true;childRuns++;}
function bodyStartedParent(){output.push("before");child();output.push("after");}
function writeParent(){const guarded=process.env.SNAP21_REPLAY_VALUE;durableCell++;child();return guarded;}
if(wrong==="R1" || wrong===undefined)bodyStartedParent();
if(wrong==="R2" || wrong===undefined)writeParent();
const valid=childRuns===1 && output.join(",")===(wrong==="R2"?"":"before,after") && durableCell===(wrong==="R1"?0:1);
console.log(valid?"EQUIVALENT":"REPEATED_EFFECT",childRuns,output.join(","),durableCell);
if(!valid)process.exit(1);
