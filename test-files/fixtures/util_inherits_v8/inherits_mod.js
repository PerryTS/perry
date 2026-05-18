// V8-fallback module exercising util.inherits + the legacy
// `require('stream')` Stream constructor. Mirrors the
// `node_modules/send/index.js` pattern that broke express compile
// before stream.prototype + util.inherits-super_ were scaffolded.
//
// This file lives outside compilePackages, so Perry routes it through
// the QuickJS / V8 fallback module loader — same path express's deps
// take in production.

var util = require('util');
var Stream = require('stream');

function SendStream() {}
util.inherits(SendStream, Stream);

function Foo() {}
Foo.prototype.greet = function () {
    return 'hello';
};
function Bar() {}
util.inherits(Bar, Foo);

module.exports = {
    typeofStream: typeof Stream,
    typeofStreamProto: typeof Stream.prototype,
    typeofStreamReadable: typeof Stream.Readable,
    typeofStreamReadableProto: typeof Stream.Readable.prototype,
    sendStreamSuper: SendStream.super_ === Stream,
    barInstanceofFoo: new Bar() instanceof Foo,
    barGreet: new Bar().greet(),
    barSuper: Bar.super_ === Foo,
};
