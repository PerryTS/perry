'use strict';
// A CommonJS EventEmitter subclass in both forms node packages use:
// `class extends require("events").EventEmitter` and util.inherits.
const events = require('events');
const util = require('util');
class Bus extends events.EventEmitter {
  publish(topic, v) { return this.emit(topic, v); }
}
function Legacy() { events.EventEmitter.call(this); this.seen = []; }
util.inherits(Legacy, events.EventEmitter);
Legacy.prototype.feed = function (v) { this.emit('data', v); };
exports.Bus = Bus;
exports.Legacy = Legacy;
