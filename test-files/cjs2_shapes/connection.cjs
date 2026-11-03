'use strict';
// pg/lib/connection.js shape: a CommonJS EventEmitter subclass.
const EventEmitter = require('events').EventEmitter;
class Connection extends EventEmitter {
  constructor(config) {
    super();
    this.config = config || {};
    this._ending = false;
  }
  connect() {
    this.emit('connect');
    this.emit('message', { name: 'readyForQuery', status: 'I' });
  }
  sendError(text) {
    this.emit('errorMessage', { name: 'error', message: text, code: '42P01' });
  }
  end() {
    this._ending = true;
    this.emit('end');
  }
}
module.exports = Connection;
