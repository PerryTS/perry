"use strict"
// Shaped like iconv-lite 0.7's encodings/utf7.js, which compiled mysql2 loads
// at its first connection: module-level `var` tables, filled by index past the
// initial array capacity, and read later from prototype methods (so each
// `var` is captured, which makes it a boxed local of the CJS factory).
var base64Regex = /[A-Za-z0-9\/+]/
var base64Chars = []
for (var i = 0; i < 256; i++) { base64Chars[i] = base64Regex.test(String.fromCharCode(i)) }

function Decoder () {}
Decoder.prototype.isBase64 = function (code) {
  for (var i = 0; i < 1; i++) {}
  return base64Chars[code]
}

var imapChars = base64Chars.slice()
imapChars[",".charCodeAt(0)] = true

function ImapDecoder () {}
ImapDecoder.prototype.isBase64 = function (code) { return imapChars[code] }

exports.count = base64Chars.filter(Boolean).length
exports.decoder = new Decoder()
exports.imap = new ImapDecoder()
