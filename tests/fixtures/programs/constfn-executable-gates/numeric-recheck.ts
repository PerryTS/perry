import { mutateNumericReceiver } from "./numeric-recheck-control.ts";
class Pair { a=1; b=2; m=()=>this.a; }
const pair=new Pair();
let events=0;
let sink:any;
function touch():void {
    events++;
    sink={event:events};
    if(process.argv[2]==="mutate" && events===100) mutateNumericReceiver(pair);
}
function readEffects(p:Pair, visit:()=>void):number {
    let sum=0;
    for(let i=0;i<200;i++){ sum+=p.a*2+p.b*2; visit(); }
    return sum;
}
function invoke(receiver:any):number { return receiver.m(); }
const total=readEffects(pair,touch);
let methods=0;
for(let i=0;i<512;i++) methods+=invoke(pair);
console.log(total,events,methods,sink.event);
