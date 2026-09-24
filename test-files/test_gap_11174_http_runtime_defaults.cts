const http = require('node:http');
console.log('http', typeof http.createServer, typeof http.request);
console.log('regex-methods', typeof RegExp.prototype.test, typeof RegExp.prototype.exec);
const pattern = new RegExp('a(b+)');
console.log('regex', pattern.test('abb'), pattern.exec('abb')[1]);
console.log('url', new URL('https://example.org/path?q=1').hostname);
console.log('normalize', 'e\u0301'.normalize('NFC') === '\u00e9');
console.log('temporal', typeof Temporal.Instant.from);
