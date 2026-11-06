// Test Date methods: parse, UTC, setters, formatting, getDay, valueOf

// Date.parse
const parsed = Date.parse("2024-01-15T00:00:00.000Z");
console.log(parsed); // 1705276800000

// Date.UTC
const utc = Date.UTC(2024, 0, 15);
console.log(utc); // 1705276800000

// Date.parse and Date.UTC should agree for the same UTC date
console.log(parsed === utc); // true

// Create a known date in UTC for setter/getter tests
const d = new Date(Date.UTC(2024, 0, 15, 12, 30, 45));

// getDay (day of week: Monday = 1 for Jan 15 2024)
console.log(d.getUTCDay()); // 1

// valueOf same as getTime
console.log(d.valueOf() === d.getTime()); // true

// setFullYear / getFullYear
const d2 = new Date(Date.UTC(2024, 0, 15));
d2.setUTCFullYear(2025);
console.log(d2.getUTCFullYear()); // 2025

// setMonth / getMonth (0-based)
d2.setUTCMonth(5);
console.log(d2.getUTCMonth()); // 5

// setDate / getDate
d2.setUTCDate(20);
console.log(d2.getUTCDate()); // 20

// setHours / getHours
d2.setUTCHours(14);
console.log(d2.getUTCHours()); // 14

// setMinutes / getMinutes
d2.setUTCMinutes(45);
console.log(d2.getUTCMinutes()); // 45

// setSeconds / getSeconds
d2.setUTCSeconds(30);
console.log(d2.getUTCSeconds()); // 30

// toDateString returns a human-readable date string
const d3 = new Date(Date.UTC(2024, 0, 15, 12, 0, 0));
const dateStr = d3.toDateString();
console.log(typeof dateStr); // string
console.log(dateStr.includes("2024")); // true

// toTimeString returns a time string
const timeStr = d3.toTimeString();
console.log(typeof timeStr); // string
console.log(timeStr.includes(":")); // true

// toLocaleDateString and toLocaleTimeString return strings
const localeDate = d3.toLocaleDateString();
console.log(typeof localeDate); // string
console.log(localeDate.length > 0); // true

const localeTime = d3.toLocaleTimeString();
console.log(typeof localeTime); // string
console.log(localeTime.length > 0); // true

// getTimezoneOffset returns a number
const tzOffset = d3.getTimezoneOffset();
console.log(typeof tzOffset); // number

// toJSON returns ISO string
const json = d3.toJSON();
console.log(json); // 2024-01-15T12:00:00.000Z

// toISOString matches toJSON
console.log(d3.toISOString() === d3.toJSON()); // true

// Expected output:
// 1705276800000
// 1705276800000
// true
// 1
// true
// 2025
// 5
// 20
// 14
// 45
// 30
// string
// true
// string
// true
// string
// true
// string
// true
// number
// 2024-01-15T12:00:00.000Z
// true
