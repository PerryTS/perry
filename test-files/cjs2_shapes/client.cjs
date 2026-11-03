'use strict';
// pg/lib/client.js shape: a Client that owns a Connection and deletes
// its own query bookkeeping by a computed key.
const EventEmitter = require('events').EventEmitter;
const Connection = require('./connection.cjs');
class Client extends EventEmitter {
  constructor(config) {
    super();
    this.connection = new Connection(config);
    this.pending = { q1: 'select 1', q2: 'select 2' };
    this.log = [];
    this._attachListeners(this.connection);
  }
  _attachListeners(con) {
    con.on('connect', () => this.log.push('connect'));
    con.on('message', (m) => this.log.push('message:' + m.name + ':' + m.status));
    con.on('errorMessage', this._handleErrorMessage.bind(this));
    con.on('end', () => this.emit('end'));
  }
  _handleErrorMessage(msg) {
    for (const k of Object.keys(this.pending)) {
      if (this.pending[k].indexOf('2') >= 0) delete this.pending[k];
    }
    this.log.push('error:' + msg.code + ':' + msg.message);
    this.emit('error', new Error(msg.message));
  }
  connect() { this.connection.connect(); return this; }
  end() { this.connection.end(); }
}
module.exports = Client;
