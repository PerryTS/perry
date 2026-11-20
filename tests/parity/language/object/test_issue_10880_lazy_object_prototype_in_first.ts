// `in` must find inherited Object.prototype members even when nothing has
// materialized the realm's Object constructor yet.
const o: any = { x: 1 };
console.log("toString" in o, "hasOwnProperty" in o, "valueOf" in o);
console.log(typeof o.toString, typeof o.hasOwnProperty);
