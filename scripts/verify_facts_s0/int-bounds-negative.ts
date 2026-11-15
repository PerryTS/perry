// `h` leaves the int32 range at its first product (1.98e18) and the i64
// range at the second (3.9e36). With `PERRY_FACTS_INT_BOUNDS=0` the interval
// rules give a product past 2^60 the empty interval, so `h` keeps the
// interval of its literal and is claimed an int32 (as is the key `s`, whose
// shift result 2^62 was empty too): the store witness of the int32 proof
// must report the product. With the bounds of doubles the first product is
// "an integer at or past 2^53", `h` is no int32 and nothing is claimed.
const a: number[] = [10, 11, 12, 13];
let h = 1406932606;
h *= h;
h *= h;
let s = 2147483647;
s <<= 31;
console.log(~h, ~h & 0xffff, ~~h, (~h + 1) | 0, a[s], s);
