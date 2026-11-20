
const N = 1;
function check(name: string, make: (i: number) => any, invoke?: (o: any) => any): void {
  let bad = 0;
  const notes: string[] = [];
  for (let i = 0; i < N; i++) {
    const o: any = make(i);
    const want = "tag" + i + ":2000";
    let got: any;
    try {
      got = invoke ? invoke(o) : o.run();
    } catch (e: any) {
      got = "THREW:" + (e && e.message);
    }
    if (got !== want) {
      bad++;
      if (bad <= 2) notes.push(" [" + i + " got=" + String(got) + "]");
    }
  }
  console.log(name + " bad=" + bad + notes.join(""));
}
function run(this: any): string {
  const n = this.churn(); // dynamic method call; a moving minor runs inside it
  return this.tag + ":" + n; // `this` must still be the (moved) receiver
}
function runViaCall(this: any): string {
  const n = this.churn.call(this);
  return this.tag + ":" + n;
}
function runViaApply(this: any): string {
  const n = this.churn.apply(this, []);
  return this.tag + ":" + n;
}
const proto: any = {
  churn: function (this: any): number {
    return churn();
  },
  run: run,
  runViaCall: runViaCall,
  runViaApply: runViaApply,
};
check(
  "object_create",
  (i) => {
    const o = Object.create(proto);
    o.tag = "tag" + i;
    return o;
  },
);
class Base {
  tag: string;
  constructor(i: number) {
    this.tag = "tag" + i;
  }
}
Object.assign(Base.prototype, { churn: proto.churn, run: run });
