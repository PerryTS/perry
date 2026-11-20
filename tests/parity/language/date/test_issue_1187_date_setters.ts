// Issue #1187 — Date.prototype local-time setters
// Pre-fix: `d.setHours(...)` threw `TypeError: (number).setHours is not a function`
// because the HIR lowering only dispatched `setUTC*` methods.

function getExpiry(): Date {
  const expiry = new Date(2024, 0, 15, 10, 0, 0, 0);
  expiry.setHours(expiry.getHours() + 1);
  return expiry;
}

const e = getExpiry();
console.log(e.getHours());

const d = new Date(2024, 0, 15, 10, 0, 0, 0);
d.setDate(20);
console.log(d.getDate());
d.setMonth(5);
console.log(d.getMonth());
d.setFullYear(2025);
console.log(d.getFullYear());
d.setMinutes(30);
console.log(d.getMinutes());
d.setSeconds(45);
console.log(d.getSeconds());
d.setMilliseconds(500);
console.log(d.getMilliseconds());
d.setTime(0);
console.log(d.getTime());

const d2 = new Date(2024, 0, 15, 10, 0, 0, 0);
console.log(d2.setHours(15));
console.log(d2.getHours());
