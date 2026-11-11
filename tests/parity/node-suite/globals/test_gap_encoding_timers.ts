// Test encoding APIs (TextEncoder, TextDecoder, encodeURI/decodeURI) and timer APIs (clearTimeout, clearInterval)

// === TextEncoder ===
const encoder = new TextEncoder();
const encoded = encoder.encode("hello");
console.log("TextEncoder type:", encoded instanceof Uint8Array); // TextEncoder type: true
console.log("TextEncoder length:", encoded.length); // TextEncoder length: 5
console.log("TextEncoder bytes:", encoded[0], encoded[1], encoded[2], encoded[3], encoded[4]); // TextEncoder bytes: 104 101 108 108 111

// Encode empty string
const emptyEncoded = encoder.encode("");
console.log("empty encode length:", emptyEncoded.length); // empty encode length: 0

// Encode Unicode
const unicodeEncoded = encoder.encode("\u00e9"); // e-acute (2 UTF-8 bytes)
console.log("unicode encode length:", unicodeEncoded.length); // unicode encode length: 2
console.log("unicode bytes:", unicodeEncoded[0], unicodeEncoded[1]); // unicode bytes: 195 169

// Encode emoji (4 UTF-8 bytes)
const emojiEncoded = encoder.encode("\u{1F600}");
console.log("emoji encode length:", emojiEncoded.length); // emoji encode length: 4

// TextEncoder encoding property
console.log("encoder.encoding:", encoder.encoding); // encoder.encoding: utf-8

// === TextDecoder ===
const decoder = new TextDecoder();
const decoded = decoder.decode(new Uint8Array([104, 101, 108, 108, 111]));
console.log("TextDecoder:", decoded); // TextDecoder: hello

// Decode empty
const emptyDecoded = decoder.decode(new Uint8Array([]));
console.log("empty decode:", emptyDecoded); // empty decode:

// Decode UTF-8 multi-byte
const multiByteDecoded = decoder.decode(new Uint8Array([195, 169])); // e-acute
console.log("multibyte decode:", multiByteDecoded); // multibyte decode: é
console.log("multibyte codepoint:", multiByteDecoded.charCodeAt(0)); // multibyte codepoint: 233

// TextDecoder with explicit encoding
const utf8Decoder = new TextDecoder("utf-8");
const utf8Decoded = utf8Decoder.decode(new Uint8Array([72, 101, 108, 108, 111]));
console.log("utf-8 decode:", utf8Decoded); // utf-8 decode: Hello

// TextDecoder encoding property
console.log("decoder.encoding:", decoder.encoding); // decoder.encoding: utf-8

// Round-trip: encode then decode
const originalText = "Hello, World! \u00e9\u00e8\u00ea";
const roundTrip = new TextDecoder().decode(new TextEncoder().encode(originalText));
console.log("round-trip match:", roundTrip === originalText); // round-trip match: true

// === encodeURI / decodeURI ===
// encodeURI preserves URI structure characters (: / ? # etc.)
const uri = "https://example.com/path?q=hello world&lang=en";
const encodedUri = encodeURI(uri);
console.log("encodeURI:", encodedUri); // encodeURI: https://example.com/path?q=hello%20world&lang=en

// decodeURI reverses it
const decodedUri = decodeURI(encodedUri);
console.log("decodeURI:", decodedUri); // decodeURI: https://example.com/path?q=hello world&lang=en

// encodeURI vs encodeURIComponent
const component = "hello world&foo=bar";
const uriEncoded = encodeURI(component);
const componentEncoded = encodeURIComponent(component);
console.log("encodeURI component:", uriEncoded); // encodeURI component: hello%20world&foo=bar
console.log("encodeURIComponent:", componentEncoded); // encodeURIComponent: hello%20world%26foo%3Dbar

// encodeURI with Unicode
const unicodeUri = "https://example.com/\u00e9";
const encodedUnicodeUri = encodeURI(unicodeUri);
console.log("encodeURI unicode:", encodedUnicodeUri); // encodeURI unicode: https://example.com/%C3%A9

// decodeURI round-trip
console.log("URI round-trip:", decodeURI(encodeURI(uri)) === uri); // URI round-trip: true

// decodeURIComponent round-trip
console.log("component round-trip:", decodeURIComponent(encodeURIComponent(component)) === component); // component round-trip: true

// === clearTimeout ===
// Set a timeout and then clear it — callback should NOT fire
let timeoutFired = false;
const timeoutId = setTimeout(() => {
  timeoutFired = true;
}, 50);
console.log("timeout ID type:", typeof timeoutId); // timeout ID type: object
clearTimeout(timeoutId);

// Wait long enough for the timeout to have fired if it wasn't cleared
await new Promise<void>(resolve => setTimeout(resolve, 200));
console.log("cleared timeout fired:", timeoutFired); // cleared timeout fired: false

// setTimeout returns a value (timer ID)
const id1 = setTimeout(() => {}, 1000);
const id2 = setTimeout(() => {}, 1000);
console.log("timer IDs are defined:", id1 !== undefined && id2 !== undefined); // timer IDs are defined: true
clearTimeout(id1);
clearTimeout(id2);

// clearTimeout with invalid/already-cleared ID is a no-op
clearTimeout(undefined as any);
clearTimeout(null as any);
console.log("clearTimeout invalid: ok"); // clearTimeout invalid: ok

// === clearInterval ===
let intervalCount = 0;
const intervalId = setInterval(() => {
  intervalCount++;
}, 30);

// Let it tick a couple times
await new Promise<void>(resolve => setTimeout(resolve, 100));
clearInterval(intervalId);
const countAfterClear = intervalCount;
console.log("interval ran:", intervalCount > 0); // interval ran: true

// Wait a bit more to confirm it stopped
await new Promise<void>(resolve => setTimeout(resolve, 100));
console.log("interval stopped:", intervalCount === countAfterClear); // interval stopped: true

// clearInterval with undefined is a no-op
clearInterval(undefined as any);
console.log("clearInterval invalid: ok"); // clearInterval invalid: ok

// === setTimeout with 0 delay ===
let zeroDelayRan = false;
setTimeout(() => {
  zeroDelayRan = true;
}, 0);
// Should not have run synchronously
console.log("zero delay sync:", zeroDelayRan); // zero delay sync: false
await new Promise<void>(resolve => setTimeout(resolve, 10));
console.log("zero delay after tick:", zeroDelayRan); // zero delay after tick: true

// === setTimeout returns value ===
const result = await new Promise<string>(resolve => {
  setTimeout(() => resolve("timer result"), 10);
});
console.log("promise from timeout:", result); // promise from timeout: timer result

console.log("All encoding and timer tests passed!"); // All encoding and timer tests passed!

// Expected output:
// TextEncoder type: true
// TextEncoder length: 5
// TextEncoder bytes: 104 101 108 108 111
// empty encode length: 0
// unicode encode length: 2
// unicode bytes: 195 169
// emoji encode length: 4
// encoder.encoding: utf-8
// TextDecoder: hello
// empty decode:
// multibyte decode: é
// multibyte codepoint: 233
// utf-8 decode: Hello
// decoder.encoding: utf-8
// round-trip match: true
// encodeURI: https://example.com/path?q=hello%20world&lang=en
// decodeURI: https://example.com/path?q=hello world&lang=en
// encodeURI component: hello%20world&foo=bar
// encodeURIComponent: hello%20world%26foo%3Dbar
// encodeURI unicode: https://example.com/%C3%A9
// URI round-trip: true
// component round-trip: true
// timeout ID type: object
// cleared timeout fired: false
// timer IDs are defined: true
// clearTimeout invalid: ok
// interval ran: true
// interval stopped: true
// clearInterval invalid: ok
// zero delay sync: false
// zero delay after tick: true
// promise from timeout: timer result
// All encoding and timer tests passed!
