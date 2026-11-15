// A method put on the prototype of a constructor a host object holds, read
// by key on an instance. Without the realm table's `held` rows
// (`noreachheldkeys`) `Temporal.Duration.prototype` reads as absent, the
// store has no receiver, and the method's body is called inactive.
function read(o: any, k: string): any { return o[k]; }
const duration: any = new Temporal.Duration(1, 2, 3, 4);
(Temporal.Duration.prototype as any).describeYears = function (this: any) { return "y=" + this.years; };
console.log(read(duration, "describeYears").call(duration));
