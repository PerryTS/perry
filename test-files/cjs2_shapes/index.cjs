'use strict';
// pg/lib/index.js shape: a constructor function that stores the classes
// on its instance, and a lazy getter for the optional native half.
const Client = require('./client.cjs');
const Connection = require('./connection.cjs');
const PG = function (clientConstructor) {
  this.defaults = { host: 'localhost', port: 5432 };
  this.Client = clientConstructor;
  this.Connection = Connection;
  this.types = { getTypeParser: (oid) => (v) => oid + ':' + v };
};
module.exports = new PG(Client);
let nativeLoads = 0;
Object.defineProperty(module.exports, 'native', {
  configurable: true,
  enumerable: false,
  get() {
    nativeLoads++;
    const native = { loaded: nativeLoads };
    Object.defineProperty(module.exports, 'native', { value: native });
    return native;
  },
});
