// Next.js stream-ops.node.js shape: the stream namespace from a CommonJS require.
"use strict";
const _nodestream = require("node:stream");
function nodeReadableToWebReadableStream(stream) {
  if (stream instanceof ReadableStream) return stream;
  return _nodestream.Readable.toWeb(stream);
}
exports.toWeb = nodeReadableToWebReadableStream;
exports.from = (chunks) => _nodestream.Readable.from(chunks.map((c) => Buffer.from(c)));
