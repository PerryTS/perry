'use strict';
// CommonJS half of test_gap_10428_10429_node_module_value_dispatch.ts: the
// exact shapes pg, mysql2, ioredis and ws use when compiled from source.
const net = require('net');
const http = require('http');

exports.sameModules = function () {
  return [require('node:net') === net, require('node:http') === http];
};
// mysql2 lib/base/connection.js: `Net.connect(port, host)`
exports.connectMember = function (port, host) {
  return net.connect(port, host);
};
// ioredis/iovalkey StandaloneConnector: `(0, net_1.createConnection)(options)`
exports.createConnectionCall = function (options) {
  return (0, net.createConnection)(options);
};
// pg lib/stream.js: function-local require, then `new net.Socket()`
exports.newLocalSocket = function () {
  const n2 = require('net');
  return new n2.Socket();
};
// pg lib/connection.js: `net.isIP(host)`
exports.isIP = function (host) {
  return net.isIP(host);
};
// fastify lib/server.js: `http.createServer(options.http, httpHandler)`
exports.createServer = function (options, handler) {
  return http.createServer(options, handler);
};
// ws lib/websocket.js: `const request = isSecure ? https.request : http.request`
exports.request = function (options, callback) {
  const request = http.request;
  return request(options, callback);
};
