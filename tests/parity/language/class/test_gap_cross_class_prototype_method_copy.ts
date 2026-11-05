// Copying a method value from one class's prototype onto another's — the block
// copy mysql2's Command state machine performs
// (`Execute.prototype.resultsetHeader = Query.prototype.resultsetHeader`).
//
// A method value is a FIXED function object: invoking it must run the OWNER's
// body with the call-time `this`, never re-resolve the method name against the
// receiver's own class. Re-resolving finds the copy itself and recurses until
// the call-depth guard yields the null object (`[object Object]`).

class Query {
  tag = "Query";
  resultsetHeader(this: any, n: number): any {
    return "Q.rsh:" + this.tag + ":" + n;
  }
  done(this: any): any {
    return "Q.done:" + this.tag;
  }
}

class Execute {
  tag = "Execute";
  start(this: any): any {
    return (Execute as any).prototype.resultsetHeader;
  }
  // Execute has its OWN readField; the copied Query method reads it off `this`,
  // which must still resolve virtually to Execute's override.
  readField(this: any): any {
    return "Execute.readField";
  }
}

(Execute as any).prototype.resultsetHeader = (Query as any).prototype.resultsetHeader;
(Execute as any).prototype.done = (Query as any).prototype.done;

const ex: any = new Execute();

// direct call through the copied prototype slot
console.log("direct      :", ex.resultsetHeader(6));
console.log("copied done :", ex.done());

// as a state-machine value: read it, then invoke it later (mysql2's `this.next`)
const next = ex.start();
console.log("typeof next :", typeof next);
console.log("invoke next :", next.call(ex, 6));

// the value read straight off the source prototype
const q: any = new Query();
const m = (Query as any).prototype.resultsetHeader;
console.log("proto value :", typeof m, m.call(q, 1));

// an override on the receiver must NOT hijack the copied owner's body
class Sub extends Query {
  resultsetHeader(this: any, n: number): any {
    return "Sub.rsh:" + n;
  }
}
const sub: any = new Sub();
console.log("override    :", sub.resultsetHeader(2));
console.log("owner body  :", (Query as any).prototype.resultsetHeader.call(sub, 3));

// method identity: reading a method twice yields the same function object
console.log(
  "identity    :",
  (Query as any).prototype.done === (Query as any).prototype.done,
);
