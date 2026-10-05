import require$$0$1 from 'net';
import require$$1 from 'tls';
import assert from 'assert';
import require$$2 from 'url';
import require$$0 from 'tty';
import require$$0__default from 'util';
import require$$0__default$1 from 'events';
import require$$2__default from 'http';
import require$$1__default from 'https';

var dist$2 = {};

var src = {exports: {}};

var browser = {exports: {}};

/**
 * Helpers.
 */

var ms;
var hasRequiredMs;

function requireMs () {
	if (hasRequiredMs) return ms;
	hasRequiredMs = 1;
	var s = 1000;
	var m = s * 60;
	var h = m * 60;
	var d = h * 24;
	var w = d * 7;
	var y = d * 365.25;

	/**
	 * Parse or format the given `val`.
	 *
	 * Options:
	 *
	 *  - `long` verbose formatting [false]
	 *
	 * @param {String|Number} val
	 * @param {Object} [options]
	 * @throws {Error} throw an error if val is not a non-empty string or a number
	 * @return {String|Number}
	 * @api public
	 */

	ms = function (val, options) {
	  options = options || {};
	  var type = typeof val;
	  if (type === 'string' && val.length > 0) {
	    return parse(val);
	  } else if (type === 'number' && isFinite(val)) {
	    return options.long ? fmtLong(val) : fmtShort(val);
	  }
	  throw new Error(
	    'val is not a non-empty string or a valid number. val=' +
	      JSON.stringify(val)
	  );
	};

	/**
	 * Parse the given `str` and return milliseconds.
	 *
	 * @param {String} str
	 * @return {Number}
	 * @api private
	 */

	function parse(str) {
	  str = String(str);
	  if (str.length > 100) {
	    return;
	  }
	  var match = /^(-?(?:\d+)?\.?\d+) *(milliseconds?|msecs?|ms|seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w|years?|yrs?|y)?$/i.exec(
	    str
	  );
	  if (!match) {
	    return;
	  }
	  var n = parseFloat(match[1]);
	  var type = (match[2] || 'ms').toLowerCase();
	  switch (type) {
	    case 'years':
	    case 'year':
	    case 'yrs':
	    case 'yr':
	    case 'y':
	      return n * y;
	    case 'weeks':
	    case 'week':
	    case 'w':
	      return n * w;
	    case 'days':
	    case 'day':
	    case 'd':
	      return n * d;
	    case 'hours':
	    case 'hour':
	    case 'hrs':
	    case 'hr':
	    case 'h':
	      return n * h;
	    case 'minutes':
	    case 'minute':
	    case 'mins':
	    case 'min':
	    case 'm':
	      return n * m;
	    case 'seconds':
	    case 'second':
	    case 'secs':
	    case 'sec':
	    case 's':
	      return n * s;
	    case 'milliseconds':
	    case 'millisecond':
	    case 'msecs':
	    case 'msec':
	    case 'ms':
	      return n;
	    default:
	      return undefined;
	  }
	}

	/**
	 * Short format for `ms`.
	 *
	 * @param {Number} ms
	 * @return {String}
	 * @api private
	 */

	function fmtShort(ms) {
	  var msAbs = Math.abs(ms);
	  if (msAbs >= d) {
	    return Math.round(ms / d) + 'd';
	  }
	  if (msAbs >= h) {
	    return Math.round(ms / h) + 'h';
	  }
	  if (msAbs >= m) {
	    return Math.round(ms / m) + 'm';
	  }
	  if (msAbs >= s) {
	    return Math.round(ms / s) + 's';
	  }
	  return ms + 'ms';
	}

	/**
	 * Long format for `ms`.
	 *
	 * @param {Number} ms
	 * @return {String}
	 * @api private
	 */

	function fmtLong(ms) {
	  var msAbs = Math.abs(ms);
	  if (msAbs >= d) {
	    return plural(ms, msAbs, d, 'day');
	  }
	  if (msAbs >= h) {
	    return plural(ms, msAbs, h, 'hour');
	  }
	  if (msAbs >= m) {
	    return plural(ms, msAbs, m, 'minute');
	  }
	  if (msAbs >= s) {
	    return plural(ms, msAbs, s, 'second');
	  }
	  return ms + ' ms';
	}

	/**
	 * Pluralization helper.
	 */

	function plural(ms, msAbs, n, name) {
	  var isPlural = msAbs >= n * 1.5;
	  return Math.round(ms / n) + ' ' + name + (isPlural ? 's' : '');
	}
	return ms;
}

var common;
var hasRequiredCommon;

function requireCommon () {
	if (hasRequiredCommon) return common;
	hasRequiredCommon = 1;
	/**
	 * This is the common logic for both the Node.js and web browser
	 * implementations of `debug()`.
	 */

	function setup(env) {
		createDebug.debug = createDebug;
		createDebug.default = createDebug;
		createDebug.coerce = coerce;
		createDebug.disable = disable;
		createDebug.enable = enable;
		createDebug.enabled = enabled;
		createDebug.humanize = requireMs();
		createDebug.destroy = destroy;

		Object.keys(env).forEach(key => {
			createDebug[key] = env[key];
		});

		/**
		* The currently active debug mode names, and names to skip.
		*/

		createDebug.names = [];
		createDebug.skips = [];

		/**
		* Map of special "%n" handling functions, for the debug "format" argument.
		*
		* Valid key names are a single, lower or upper-case letter, i.e. "n" and "N".
		*/
		createDebug.formatters = {};

		/**
		* Selects a color for a debug namespace
		* @param {String} namespace The namespace string for the debug instance to be colored
		* @return {Number|String} An ANSI color code for the given namespace
		* @api private
		*/
		function selectColor(namespace) {
			let hash = 0;

			for (let i = 0; i < namespace.length; i++) {
				hash = ((hash << 5) - hash) + namespace.charCodeAt(i);
				hash |= 0; // Convert to 32bit integer
			}

			return createDebug.colors[Math.abs(hash) % createDebug.colors.length];
		}
		createDebug.selectColor = selectColor;

		/**
		* Create a debugger with the given `namespace`.
		*
		* @param {String} namespace
		* @return {Function}
		* @api public
		*/
		function createDebug(namespace) {
			let prevTime;
			let enableOverride = null;
			let namespacesCache;
			let enabledCache;

			function debug(...args) {
				// Disabled?
				if (!debug.enabled) {
					return;
				}

				const self = debug;

				// Set `diff` timestamp
				const curr = Number(new Date());
				const ms = curr - (prevTime || curr);
				self.diff = ms;
				self.prev = prevTime;
				self.curr = curr;
				prevTime = curr;

				args[0] = createDebug.coerce(args[0]);

				if (typeof args[0] !== 'string') {
					// Anything else let's inspect with %O
					args.unshift('%O');
				}

				// Apply any `formatters` transformations
				let index = 0;
				args[0] = args[0].replace(/%([a-zA-Z%])/g, (match, format) => {
					// If we encounter an escaped % then don't increase the array index
					if (match === '%%') {
						return '%';
					}
					index++;
					const formatter = createDebug.formatters[format];
					if (typeof formatter === 'function') {
						const val = args[index];
						match = formatter.call(self, val);

						// Now we need to remove `args[index]` since it's inlined in the `format`
						args.splice(index, 1);
						index--;
					}
					return match;
				});

				// Apply env-specific formatting (colors, etc.)
				createDebug.formatArgs.call(self, args);

				const logFn = self.log || createDebug.log;
				logFn.apply(self, args);
			}

			debug.namespace = namespace;
			debug.useColors = createDebug.useColors();
			debug.color = createDebug.selectColor(namespace);
			debug.extend = extend;
			debug.destroy = createDebug.destroy; // XXX Temporary. Will be removed in the next major release.

			Object.defineProperty(debug, 'enabled', {
				enumerable: true,
				configurable: false,
				get: () => {
					if (enableOverride !== null) {
						return enableOverride;
					}
					if (namespacesCache !== createDebug.namespaces) {
						namespacesCache = createDebug.namespaces;
						enabledCache = createDebug.enabled(namespace);
					}

					return enabledCache;
				},
				set: v => {
					enableOverride = v;
				}
			});

			// Env-specific initialization logic for debug instances
			if (typeof createDebug.init === 'function') {
				createDebug.init(debug);
			}

			return debug;
		}

		function extend(namespace, delimiter) {
			const newDebug = createDebug(this.namespace + (typeof delimiter === 'undefined' ? ':' : delimiter) + namespace);
			newDebug.log = this.log;
			return newDebug;
		}

		/**
		* Enables a debug mode by namespaces. This can include modes
		* separated by a colon and wildcards.
		*
		* @param {String} namespaces
		* @api public
		*/
		function enable(namespaces) {
			createDebug.save(namespaces);
			createDebug.namespaces = namespaces;

			createDebug.names = [];
			createDebug.skips = [];

			const split = (typeof namespaces === 'string' ? namespaces : '')
				.trim()
				.replace(/\s+/g, ',')
				.split(',')
				.filter(Boolean);

			for (const ns of split) {
				if (ns[0] === '-') {
					createDebug.skips.push(ns.slice(1));
				} else {
					createDebug.names.push(ns);
				}
			}
		}

		/**
		 * Checks if the given string matches a namespace template, honoring
		 * asterisks as wildcards.
		 *
		 * @param {String} search
		 * @param {String} template
		 * @return {Boolean}
		 */
		function matchesTemplate(search, template) {
			let searchIndex = 0;
			let templateIndex = 0;
			let starIndex = -1;
			let matchIndex = 0;

			while (searchIndex < search.length) {
				if (templateIndex < template.length && (template[templateIndex] === search[searchIndex] || template[templateIndex] === '*')) {
					// Match character or proceed with wildcard
					if (template[templateIndex] === '*') {
						starIndex = templateIndex;
						matchIndex = searchIndex;
						templateIndex++; // Skip the '*'
					} else {
						searchIndex++;
						templateIndex++;
					}
				} else if (starIndex !== -1) { // eslint-disable-line no-negated-condition
					// Backtrack to the last '*' and try to match more characters
					templateIndex = starIndex + 1;
					matchIndex++;
					searchIndex = matchIndex;
				} else {
					return false; // No match
				}
			}

			// Handle trailing '*' in template
			while (templateIndex < template.length && template[templateIndex] === '*') {
				templateIndex++;
			}

			return templateIndex === template.length;
		}

		/**
		* Disable debug output.
		*
		* @return {String} namespaces
		* @api public
		*/
		function disable() {
			const namespaces = [
				...createDebug.names,
				...createDebug.skips.map(namespace => '-' + namespace)
			].join(',');
			createDebug.enable('');
			return namespaces;
		}

		/**
		* Returns true if the given mode name is enabled, false otherwise.
		*
		* @param {String} name
		* @return {Boolean}
		* @api public
		*/
		function enabled(name) {
			for (const skip of createDebug.skips) {
				if (matchesTemplate(name, skip)) {
					return false;
				}
			}

			for (const ns of createDebug.names) {
				if (matchesTemplate(name, ns)) {
					return true;
				}
			}

			return false;
		}

		/**
		* Coerce `val`.
		*
		* @param {Mixed} val
		* @return {Mixed}
		* @api private
		*/
		function coerce(val) {
			if (val instanceof Error) {
				return val.stack || val.message;
			}
			return val;
		}

		/**
		* XXX DO NOT USE. This is a temporary stub function.
		* XXX It WILL be removed in the next major release.
		*/
		function destroy() {
			console.warn('Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.');
		}

		createDebug.enable(createDebug.load());

		return createDebug;
	}

	common = setup;
	return common;
}

/* eslint-env browser */

var hasRequiredBrowser;

function requireBrowser () {
	if (hasRequiredBrowser) return browser.exports;
	hasRequiredBrowser = 1;
	(function (module, exports) {
		/**
		 * This is the web browser implementation of `debug()`.
		 */

		exports.formatArgs = formatArgs;
		exports.save = save;
		exports.load = load;
		exports.useColors = useColors;
		exports.storage = localstorage();
		exports.destroy = (() => {
			let warned = false;

			return () => {
				if (!warned) {
					warned = true;
					console.warn('Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.');
				}
			};
		})();

		/**
		 * Colors.
		 */

		exports.colors = [
			'#0000CC',
			'#0000FF',
			'#0033CC',
			'#0033FF',
			'#0066CC',
			'#0066FF',
			'#0099CC',
			'#0099FF',
			'#00CC00',
			'#00CC33',
			'#00CC66',
			'#00CC99',
			'#00CCCC',
			'#00CCFF',
			'#3300CC',
			'#3300FF',
			'#3333CC',
			'#3333FF',
			'#3366CC',
			'#3366FF',
			'#3399CC',
			'#3399FF',
			'#33CC00',
			'#33CC33',
			'#33CC66',
			'#33CC99',
			'#33CCCC',
			'#33CCFF',
			'#6600CC',
			'#6600FF',
			'#6633CC',
			'#6633FF',
			'#66CC00',
			'#66CC33',
			'#9900CC',
			'#9900FF',
			'#9933CC',
			'#9933FF',
			'#99CC00',
			'#99CC33',
			'#CC0000',
			'#CC0033',
			'#CC0066',
			'#CC0099',
			'#CC00CC',
			'#CC00FF',
			'#CC3300',
			'#CC3333',
			'#CC3366',
			'#CC3399',
			'#CC33CC',
			'#CC33FF',
			'#CC6600',
			'#CC6633',
			'#CC9900',
			'#CC9933',
			'#CCCC00',
			'#CCCC33',
			'#FF0000',
			'#FF0033',
			'#FF0066',
			'#FF0099',
			'#FF00CC',
			'#FF00FF',
			'#FF3300',
			'#FF3333',
			'#FF3366',
			'#FF3399',
			'#FF33CC',
			'#FF33FF',
			'#FF6600',
			'#FF6633',
			'#FF9900',
			'#FF9933',
			'#FFCC00',
			'#FFCC33'
		];

		/**
		 * Currently only WebKit-based Web Inspectors, Firefox >= v31,
		 * and the Firebug extension (any Firefox version) are known
		 * to support "%c" CSS customizations.
		 *
		 * TODO: add a `localStorage` variable to explicitly enable/disable colors
		 */

		// eslint-disable-next-line complexity
		function useColors() {
			// NB: In an Electron preload script, document will be defined but not fully
			// initialized. Since we know we're in Chrome, we'll just detect this case
			// explicitly
			if (typeof window !== 'undefined' && window.process && (window.process.type === 'renderer' || window.process.__nwjs)) {
				return true;
			}

			// Internet Explorer and Edge do not support colors.
			if (typeof navigator !== 'undefined' && navigator.userAgent && navigator.userAgent.toLowerCase().match(/(edge|trident)\/(\d+)/)) {
				return false;
			}

			let m;

			// Is webkit? http://stackoverflow.com/a/16459606/376773
			// document is undefined in react-native: https://github.com/facebook/react-native/pull/1632
			// eslint-disable-next-line no-return-assign
			return (typeof document !== 'undefined' && document.documentElement && document.documentElement.style && document.documentElement.style.WebkitAppearance) ||
				// Is firebug? http://stackoverflow.com/a/398120/376773
				(typeof window !== 'undefined' && window.console && (window.console.firebug || (window.console.exception && window.console.table))) ||
				// Is firefox >= v31?
				// https://developer.mozilla.org/en-US/docs/Tools/Web_Console#Styling_messages
				(typeof navigator !== 'undefined' && navigator.userAgent && (m = navigator.userAgent.toLowerCase().match(/firefox\/(\d+)/)) && parseInt(m[1], 10) >= 31) ||
				// Double check webkit in userAgent just in case we are in a worker
				(typeof navigator !== 'undefined' && navigator.userAgent && navigator.userAgent.toLowerCase().match(/applewebkit\/(\d+)/));
		}

		/**
		 * Colorize log arguments if enabled.
		 *
		 * @api public
		 */

		function formatArgs(args) {
			args[0] = (this.useColors ? '%c' : '') +
				this.namespace +
				(this.useColors ? ' %c' : ' ') +
				args[0] +
				(this.useColors ? '%c ' : ' ') +
				'+' + module.exports.humanize(this.diff);

			if (!this.useColors) {
				return;
			}

			const c = 'color: ' + this.color;
			args.splice(1, 0, c, 'color: inherit');

			// The final "%c" is somewhat tricky, because there could be other
			// arguments passed either before or after the %c, so we need to
			// figure out the correct index to insert the CSS into
			let index = 0;
			let lastC = 0;
			args[0].replace(/%[a-zA-Z%]/g, match => {
				if (match === '%%') {
					return;
				}
				index++;
				if (match === '%c') {
					// We only are interested in the *last* %c
					// (the user may have provided their own)
					lastC = index;
				}
			});

			args.splice(lastC, 0, c);
		}

		/**
		 * Invokes `console.debug()` when available.
		 * No-op when `console.debug` is not a "function".
		 * If `console.debug` is not available, falls back
		 * to `console.log`.
		 *
		 * @api public
		 */
		exports.log = console.debug || console.log || (() => {});

		/**
		 * Save `namespaces`.
		 *
		 * @param {String} namespaces
		 * @api private
		 */
		function save(namespaces) {
			try {
				if (namespaces) {
					exports.storage.setItem('debug', namespaces);
				} else {
					exports.storage.removeItem('debug');
				}
			} catch (error) {
				// Swallow
				// XXX (@Qix-) should we be logging these?
			}
		}

		/**
		 * Load `namespaces`.
		 *
		 * @return {String} returns the previously persisted debug modes
		 * @api private
		 */
		function load() {
			let r;
			try {
				r = exports.storage.getItem('debug') || exports.storage.getItem('DEBUG') ;
			} catch (error) {
				// Swallow
				// XXX (@Qix-) should we be logging these?
			}

			// If debug isn't set in LS, and we're in Electron, try to load $DEBUG
			if (!r && typeof process !== 'undefined' && 'env' in process) {
				r = process.env.DEBUG;
			}

			return r;
		}

		/**
		 * Localstorage attempts to return the localstorage.
		 *
		 * This is necessary because safari throws
		 * when a user disables cookies/localstorage
		 * and you attempt to access it.
		 *
		 * @return {LocalStorage}
		 * @api private
		 */

		function localstorage() {
			try {
				// TVMLKit (Apple TV JS Runtime) does not have a window object, just localStorage in the global context
				// The Browser also has localStorage in the global context.
				return localStorage;
			} catch (error) {
				// Swallow
				// XXX (@Qix-) should we be logging these?
			}
		}

		module.exports = requireCommon()(exports);

		const {formatters} = module.exports;

		/**
		 * Map %j to `JSON.stringify()`, since no Web Inspectors do that by default.
		 */

		formatters.j = function (v) {
			try {
				return JSON.stringify(v);
			} catch (error) {
				return '[UnexpectedJSONParseError]: ' + error.message;
			}
		}; 
	} (browser, browser.exports));
	return browser.exports;
}

var node = {exports: {}};

/**
 * Module dependencies.
 */

var hasRequiredNode;

function requireNode () {
	if (hasRequiredNode) return node.exports;
	hasRequiredNode = 1;
	(function (module, exports) {
		const tty = require$$0;
		const util = require$$0__default;

		/**
		 * This is the Node.js implementation of `debug()`.
		 */

		exports.init = init;
		exports.log = log;
		exports.formatArgs = formatArgs;
		exports.save = save;
		exports.load = load;
		exports.useColors = useColors;
		exports.destroy = util.deprecate(
			() => {},
			'Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.'
		);

		/**
		 * Colors.
		 */

		exports.colors = [6, 2, 3, 4, 5, 1];

		try {
			// Optional dependency (as in, doesn't need to be installed, NOT like optionalDependencies in package.json)
			// eslint-disable-next-line import/no-extraneous-dependencies
			const supportsColor = require('supports-color');

			if (supportsColor && (supportsColor.stderr || supportsColor).level >= 2) {
				exports.colors = [
					20,
					21,
					26,
					27,
					32,
					33,
					38,
					39,
					40,
					41,
					42,
					43,
					44,
					45,
					56,
					57,
					62,
					63,
					68,
					69,
					74,
					75,
					76,
					77,
					78,
					79,
					80,
					81,
					92,
					93,
					98,
					99,
					112,
					113,
					128,
					129,
					134,
					135,
					148,
					149,
					160,
					161,
					162,
					163,
					164,
					165,
					166,
					167,
					168,
					169,
					170,
					171,
					172,
					173,
					178,
					179,
					184,
					185,
					196,
					197,
					198,
					199,
					200,
					201,
					202,
					203,
					204,
					205,
					206,
					207,
					208,
					209,
					214,
					215,
					220,
					221
				];
			}
		} catch (error) {
			// Swallow - we only care if `supports-color` is available; it doesn't have to be.
		}

		/**
		 * Build up the default `inspectOpts` object from the environment variables.
		 *
		 *   $ DEBUG_COLORS=no DEBUG_DEPTH=10 DEBUG_SHOW_HIDDEN=enabled node script.js
		 */

		exports.inspectOpts = Object.keys(process.env).filter(key => {
			return /^debug_/i.test(key);
		}).reduce((obj, key) => {
			// Camel-case
			const prop = key
				.substring(6)
				.toLowerCase()
				.replace(/_([a-z])/g, (_, k) => {
					return k.toUpperCase();
				});

			// Coerce string value into JS value
			let val = process.env[key];
			if (/^(yes|on|true|enabled)$/i.test(val)) {
				val = true;
			} else if (/^(no|off|false|disabled)$/i.test(val)) {
				val = false;
			} else if (val === 'null') {
				val = null;
			} else {
				val = Number(val);
			}

			obj[prop] = val;
			return obj;
		}, {});

		/**
		 * Is stdout a TTY? Colored output is enabled when `true`.
		 */

		function useColors() {
			return 'colors' in exports.inspectOpts ?
				Boolean(exports.inspectOpts.colors) :
				tty.isatty(process.stderr.fd);
		}

		/**
		 * Adds ANSI color escape codes if enabled.
		 *
		 * @api public
		 */

		function formatArgs(args) {
			const {namespace: name, useColors} = this;

			if (useColors) {
				const c = this.color;
				const colorCode = '\u001B[3' + (c < 8 ? c : '8;5;' + c);
				const prefix = `  ${colorCode};1m${name} \u001B[0m`;

				args[0] = prefix + args[0].split('\n').join('\n' + prefix);
				args.push(colorCode + 'm+' + module.exports.humanize(this.diff) + '\u001B[0m');
			} else {
				args[0] = getDate() + name + ' ' + args[0];
			}
		}

		function getDate() {
			if (exports.inspectOpts.hideDate) {
				return '';
			}
			return new Date().toISOString() + ' ';
		}

		/**
		 * Invokes `util.formatWithOptions()` with the specified arguments and writes to stderr.
		 */

		function log(...args) {
			return process.stderr.write(util.formatWithOptions(exports.inspectOpts, ...args) + '\n');
		}

		/**
		 * Save `namespaces`.
		 *
		 * @param {String} namespaces
		 * @api private
		 */
		function save(namespaces) {
			if (namespaces) {
				process.env.DEBUG = namespaces;
			} else {
				// If you set a process.env field to null or undefined, it gets cast to the
				// string 'null' or 'undefined'. Just delete instead.
				delete process.env.DEBUG;
			}
		}

		/**
		 * Load `namespaces`.
		 *
		 * @return {String} returns the previously persisted debug modes
		 * @api private
		 */

		function load() {
			return process.env.DEBUG;
		}

		/**
		 * Init logic for `debug` instances.
		 *
		 * Create a new `inspectOpts` object in case `useColors` is set
		 * differently for a particular `debug` instance.
		 */

		function init(debug) {
			debug.inspectOpts = {};

			const keys = Object.keys(exports.inspectOpts);
			for (let i = 0; i < keys.length; i++) {
				debug.inspectOpts[keys[i]] = exports.inspectOpts[keys[i]];
			}
		}

		module.exports = requireCommon()(exports);

		const {formatters} = module.exports;

		/**
		 * Map %o to `util.inspect()`, all on a single line.
		 */

		formatters.o = function (v) {
			this.inspectOpts.colors = this.useColors;
			return util.inspect(v, this.inspectOpts)
				.split('\n')
				.map(str => str.trim())
				.join(' ');
		};

		/**
		 * Map %O to `util.inspect()`, allowing multiple lines if needed.
		 */

		formatters.O = function (v) {
			this.inspectOpts.colors = this.useColors;
			return util.inspect(v, this.inspectOpts);
		}; 
	} (node, node.exports));
	return node.exports;
}

/**
 * Detect Electron renderer / nwjs process, which is node, but we should
 * treat as a browser.
 */

var hasRequiredSrc;

function requireSrc () {
	if (hasRequiredSrc) return src.exports;
	hasRequiredSrc = 1;
	if (typeof process === 'undefined' || process.type === 'renderer' || process.browser === true || process.__nwjs) {
		src.exports = requireBrowser();
	} else {
		src.exports = requireNode();
	}
	return src.exports;
}

var dist$1 = {};

var helpers = {};

var hasRequiredHelpers;

function requireHelpers () {
	if (hasRequiredHelpers) return helpers;
	hasRequiredHelpers = 1;
	var __createBinding = (helpers && helpers.__createBinding) || (Object.create ? (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    var desc = Object.getOwnPropertyDescriptor(m, k);
	    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
	      desc = { enumerable: true, get: function() { return m[k]; } };
	    }
	    Object.defineProperty(o, k2, desc);
	}) : (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    o[k2] = m[k];
	}));
	var __setModuleDefault = (helpers && helpers.__setModuleDefault) || (Object.create ? (function(o, v) {
	    Object.defineProperty(o, "default", { enumerable: true, value: v });
	}) : function(o, v) {
	    o["default"] = v;
	});
	var __importStar = (helpers && helpers.__importStar) || function (mod) {
	    if (mod && mod.__esModule) return mod;
	    var result = {};
	    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
	    __setModuleDefault(result, mod);
	    return result;
	};
	Object.defineProperty(helpers, "__esModule", { value: true });
	helpers.req = helpers.json = helpers.toBuffer = void 0;
	const http = __importStar(require$$2__default);
	const https = __importStar(require$$1__default);
	async function toBuffer(stream) {
	    let length = 0;
	    const chunks = [];
	    for await (const chunk of stream) {
	        length += chunk.length;
	        chunks.push(chunk);
	    }
	    return Buffer.concat(chunks, length);
	}
	helpers.toBuffer = toBuffer;
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	async function json(stream) {
	    const buf = await toBuffer(stream);
	    const str = buf.toString('utf8');
	    try {
	        return JSON.parse(str);
	    }
	    catch (_err) {
	        const err = _err;
	        err.message += ` (input: ${str})`;
	        throw err;
	    }
	}
	helpers.json = json;
	function req(url, opts = {}) {
	    const href = typeof url === 'string' ? url : url.href;
	    const req = (href.startsWith('https:') ? https : http).request(url, opts);
	    const promise = new Promise((resolve, reject) => {
	        req
	            .once('response', resolve)
	            .once('error', reject)
	            .end();
	    });
	    req.then = promise.then.bind(promise);
	    return req;
	}
	helpers.req = req;
	
	return helpers;
}

var hasRequiredDist$2;

function requireDist$2 () {
	if (hasRequiredDist$2) return dist$1;
	hasRequiredDist$2 = 1;
	(function (exports) {
		var __createBinding = (dist$1 && dist$1.__createBinding) || (Object.create ? (function(o, m, k, k2) {
		    if (k2 === undefined) k2 = k;
		    var desc = Object.getOwnPropertyDescriptor(m, k);
		    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
		      desc = { enumerable: true, get: function() { return m[k]; } };
		    }
		    Object.defineProperty(o, k2, desc);
		}) : (function(o, m, k, k2) {
		    if (k2 === undefined) k2 = k;
		    o[k2] = m[k];
		}));
		var __setModuleDefault = (dist$1 && dist$1.__setModuleDefault) || (Object.create ? (function(o, v) {
		    Object.defineProperty(o, "default", { enumerable: true, value: v });
		}) : function(o, v) {
		    o["default"] = v;
		});
		var __importStar = (dist$1 && dist$1.__importStar) || function (mod) {
		    if (mod && mod.__esModule) return mod;
		    var result = {};
		    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
		    __setModuleDefault(result, mod);
		    return result;
		};
		var __exportStar = (dist$1 && dist$1.__exportStar) || function(m, exports) {
		    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
		};
		Object.defineProperty(exports, "__esModule", { value: true });
		exports.Agent = void 0;
		const net = __importStar(require$$0$1);
		const http = __importStar(require$$2__default);
		const https_1 = require$$1__default;
		__exportStar(requireHelpers(), exports);
		const INTERNAL = Symbol('AgentBaseInternalState');
		class Agent extends http.Agent {
		    constructor(opts) {
		        super(opts);
		        this[INTERNAL] = {};
		    }
		    /**
		     * Determine whether this is an `http` or `https` request.
		     */
		    isSecureEndpoint(options) {
		        if (options) {
		            // First check the `secureEndpoint` property explicitly, since this
		            // means that a parent `Agent` is "passing through" to this instance.
		            // eslint-disable-next-line @typescript-eslint/no-explicit-any
		            if (typeof options.secureEndpoint === 'boolean') {
		                return options.secureEndpoint;
		            }
		            // If no explicit `secure` endpoint, check if `protocol` property is
		            // set. This will usually be the case since using a full string URL
		            // or `URL` instance should be the most common usage.
		            if (typeof options.protocol === 'string') {
		                return options.protocol === 'https:';
		            }
		        }
		        // Finally, if no `protocol` property was set, then fall back to
		        // checking the stack trace of the current call stack, and try to
		        // detect the "https" module.
		        const { stack } = new Error();
		        if (typeof stack !== 'string')
		            return false;
		        return stack
		            .split('\n')
		            .some((l) => l.indexOf('(https.js:') !== -1 ||
		            l.indexOf('node:https:') !== -1);
		    }
		    // In order to support async signatures in `connect()` and Node's native
		    // connection pooling in `http.Agent`, the array of sockets for each origin
		    // has to be updated synchronously. This is so the length of the array is
		    // accurate when `addRequest()` is next called. We achieve this by creating a
		    // fake socket and adding it to `sockets[origin]` and incrementing
		    // `totalSocketCount`.
		    incrementSockets(name) {
		        // If `maxSockets` and `maxTotalSockets` are both Infinity then there is no
		        // need to create a fake socket because Node.js native connection pooling
		        // will never be invoked.
		        if (this.maxSockets === Infinity && this.maxTotalSockets === Infinity) {
		            return null;
		        }
		        // All instances of `sockets` are expected TypeScript errors. The
		        // alternative is to add it as a private property of this class but that
		        // will break TypeScript subclassing.
		        if (!this.sockets[name]) {
		            // @ts-expect-error `sockets` is readonly in `@types/node`
		            this.sockets[name] = [];
		        }
		        const fakeSocket = new net.Socket({ writable: false });
		        this.sockets[name].push(fakeSocket);
		        // @ts-expect-error `totalSocketCount` isn't defined in `@types/node`
		        this.totalSocketCount++;
		        return fakeSocket;
		    }
		    decrementSockets(name, socket) {
		        if (!this.sockets[name] || socket === null) {
		            return;
		        }
		        const sockets = this.sockets[name];
		        const index = sockets.indexOf(socket);
		        if (index !== -1) {
		            sockets.splice(index, 1);
		            // @ts-expect-error  `totalSocketCount` isn't defined in `@types/node`
		            this.totalSocketCount--;
		            if (sockets.length === 0) {
		                // @ts-expect-error `sockets` is readonly in `@types/node`
		                delete this.sockets[name];
		            }
		        }
		    }
		    // In order to properly update the socket pool, we need to call `getName()` on
		    // the core `https.Agent` if it is a secureEndpoint.
		    getName(options) {
		        const secureEndpoint = this.isSecureEndpoint(options);
		        if (secureEndpoint) {
		            // @ts-expect-error `getName()` isn't defined in `@types/node`
		            return https_1.Agent.prototype.getName.call(this, options);
		        }
		        // @ts-expect-error `getName()` isn't defined in `@types/node`
		        return super.getName(options);
		    }
		    createSocket(req, options, cb) {
		        const connectOpts = {
		            ...options,
		            secureEndpoint: this.isSecureEndpoint(options),
		        };
		        const name = this.getName(connectOpts);
		        const fakeSocket = this.incrementSockets(name);
		        Promise.resolve()
		            .then(() => this.connect(req, connectOpts))
		            .then((socket) => {
		            this.decrementSockets(name, fakeSocket);
		            if (socket instanceof http.Agent) {
		                try {
		                    // @ts-expect-error `addRequest()` isn't defined in `@types/node`
		                    return socket.addRequest(req, connectOpts);
		                }
		                catch (err) {
		                    return cb(err);
		                }
		            }
		            this[INTERNAL].currentSocket = socket;
		            // @ts-expect-error `createSocket()` isn't defined in `@types/node`
		            super.createSocket(req, options, cb);
		        }, (err) => {
		            this.decrementSockets(name, fakeSocket);
		            cb(err);
		        });
		    }
		    createConnection() {
		        const socket = this[INTERNAL].currentSocket;
		        this[INTERNAL].currentSocket = undefined;
		        if (!socket) {
		            throw new Error('No socket was returned in the `connect()` function');
		        }
		        return socket;
		    }
		    get defaultPort() {
		        return (this[INTERNAL].defaultPort ??
		            (this.protocol === 'https:' ? 443 : 80));
		    }
		    set defaultPort(v) {
		        if (this[INTERNAL]) {
		            this[INTERNAL].defaultPort = v;
		        }
		    }
		    get protocol() {
		        return (this[INTERNAL].protocol ??
		            (this.isSecureEndpoint() ? 'https:' : 'http:'));
		    }
		    set protocol(v) {
		        if (this[INTERNAL]) {
		            this[INTERNAL].protocol = v;
		        }
		    }
		}
		exports.Agent = Agent;
		
	} (dist$1));
	return dist$1;
}

var parseProxyResponse = {};

var hasRequiredParseProxyResponse;

function requireParseProxyResponse () {
	if (hasRequiredParseProxyResponse) return parseProxyResponse;
	hasRequiredParseProxyResponse = 1;
	var __importDefault = (parseProxyResponse && parseProxyResponse.__importDefault) || function (mod) {
	    return (mod && mod.__esModule) ? mod : { "default": mod };
	};
	Object.defineProperty(parseProxyResponse, "__esModule", { value: true });
	parseProxyResponse.parseProxyResponse = void 0;
	const debug_1 = __importDefault(requireSrc());
	const debug = (0, debug_1.default)('https-proxy-agent:parse-proxy-response');
	function parseProxyResponse$1(socket) {
	    return new Promise((resolve, reject) => {
	        // we need to buffer any HTTP traffic that happens with the proxy before we get
	        // the CONNECT response, so that if the response is anything other than an "200"
	        // response code, then we can re-play the "data" events on the socket once the
	        // HTTP parser is hooked up...
	        let buffersLength = 0;
	        const buffers = [];
	        function read() {
	            const b = socket.read();
	            if (b)
	                ondata(b);
	            else
	                socket.once('readable', read);
	        }
	        function cleanup() {
	            socket.removeListener('end', onend);
	            socket.removeListener('error', onerror);
	            socket.removeListener('readable', read);
	        }
	        function onend() {
	            cleanup();
	            debug('onend');
	            reject(new Error('Proxy connection ended before receiving CONNECT response'));
	        }
	        function onerror(err) {
	            cleanup();
	            debug('onerror %o', err);
	            reject(err);
	        }
	        function ondata(b) {
	            buffers.push(b);
	            buffersLength += b.length;
	            const buffered = Buffer.concat(buffers, buffersLength);
	            const endOfHeaders = buffered.indexOf('\r\n\r\n');
	            if (endOfHeaders === -1) {
	                // keep buffering
	                debug('have not received end of HTTP headers yet...');
	                read();
	                return;
	            }
	            const headerParts = buffered
	                .slice(0, endOfHeaders)
	                .toString('ascii')
	                .split('\r\n');
	            const firstLine = headerParts.shift();
	            if (!firstLine) {
	                socket.destroy();
	                return reject(new Error('No header received from proxy CONNECT response'));
	            }
	            const firstLineParts = firstLine.split(' ');
	            const statusCode = +firstLineParts[1];
	            const statusText = firstLineParts.slice(2).join(' ');
	            const headers = {};
	            for (const header of headerParts) {
	                if (!header)
	                    continue;
	                const firstColon = header.indexOf(':');
	                if (firstColon === -1) {
	                    socket.destroy();
	                    return reject(new Error(`Invalid header from proxy CONNECT response: "${header}"`));
	                }
	                const key = header.slice(0, firstColon).toLowerCase();
	                const value = header.slice(firstColon + 1).trimStart();
	                const current = headers[key];
	                if (typeof current === 'string') {
	                    headers[key] = [current, value];
	                }
	                else if (Array.isArray(current)) {
	                    current.push(value);
	                }
	                else {
	                    headers[key] = value;
	                }
	            }
	            debug('got proxy server response: %o %o', firstLine, headers);
	            cleanup();
	            resolve({
	                connect: {
	                    statusCode,
	                    statusText,
	                    headers,
	                },
	                buffered,
	            });
	        }
	        socket.on('error', onerror);
	        socket.on('end', onend);
	        read();
	    });
	}
	parseProxyResponse.parseProxyResponse = parseProxyResponse$1;
	
	return parseProxyResponse;
}

var hasRequiredDist$1;

function requireDist$1 () {
	if (hasRequiredDist$1) return dist$2;
	hasRequiredDist$1 = 1;
	var __createBinding = (dist$2 && dist$2.__createBinding) || (Object.create ? (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    var desc = Object.getOwnPropertyDescriptor(m, k);
	    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
	      desc = { enumerable: true, get: function() { return m[k]; } };
	    }
	    Object.defineProperty(o, k2, desc);
	}) : (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    o[k2] = m[k];
	}));
	var __setModuleDefault = (dist$2 && dist$2.__setModuleDefault) || (Object.create ? (function(o, v) {
	    Object.defineProperty(o, "default", { enumerable: true, value: v });
	}) : function(o, v) {
	    o["default"] = v;
	});
	var __importStar = (dist$2 && dist$2.__importStar) || function (mod) {
	    if (mod && mod.__esModule) return mod;
	    var result = {};
	    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
	    __setModuleDefault(result, mod);
	    return result;
	};
	var __importDefault = (dist$2 && dist$2.__importDefault) || function (mod) {
	    return (mod && mod.__esModule) ? mod : { "default": mod };
	};
	Object.defineProperty(dist$2, "__esModule", { value: true });
	dist$2.HttpsProxyAgent = void 0;
	const net = __importStar(require$$0$1);
	const tls = __importStar(require$$1);
	const assert_1 = __importDefault(assert);
	const debug_1 = __importDefault(requireSrc());
	const agent_base_1 = requireDist$2();
	const url_1 = require$$2;
	const parse_proxy_response_1 = requireParseProxyResponse();
	const debug = (0, debug_1.default)('https-proxy-agent');
	const setServernameFromNonIpHost = (options) => {
	    if (options.servername === undefined &&
	        options.host &&
	        !net.isIP(options.host)) {
	        return {
	            ...options,
	            servername: options.host,
	        };
	    }
	    return options;
	};
	/**
	 * The `HttpsProxyAgent` implements an HTTP Agent subclass that connects to
	 * the specified "HTTP(s) proxy server" in order to proxy HTTPS requests.
	 *
	 * Outgoing HTTP requests are first tunneled through the proxy server using the
	 * `CONNECT` HTTP request method to establish a connection to the proxy server,
	 * and then the proxy server connects to the destination target and issues the
	 * HTTP request from the proxy server.
	 *
	 * `https:` requests have their socket connection upgraded to TLS once
	 * the connection to the proxy server has been established.
	 */
	class HttpsProxyAgent extends agent_base_1.Agent {
	    constructor(proxy, opts) {
	        super(opts);
	        this.options = { path: undefined };
	        this.proxy = typeof proxy === 'string' ? new url_1.URL(proxy) : proxy;
	        this.proxyHeaders = opts?.headers ?? {};
	        debug('Creating new HttpsProxyAgent instance: %o', this.proxy.href);
	        // Trim off the brackets from IPv6 addresses
	        const host = (this.proxy.hostname || this.proxy.host).replace(/^\[|\]$/g, '');
	        const port = this.proxy.port
	            ? parseInt(this.proxy.port, 10)
	            : this.proxy.protocol === 'https:'
	                ? 443
	                : 80;
	        this.connectOpts = {
	            // Attempt to negotiate http/1.1 for proxy servers that support http/2
	            ALPNProtocols: ['http/1.1'],
	            ...(opts ? omit(opts, 'headers') : null),
	            host,
	            port,
	        };
	    }
	    /**
	     * Called when the node-core HTTP client library is creating a
	     * new HTTP request.
	     */
	    async connect(req, opts) {
	        const { proxy } = this;
	        if (!opts.host) {
	            throw new TypeError('No "host" provided');
	        }
	        // Create a socket connection to the proxy server.
	        let socket;
	        if (proxy.protocol === 'https:') {
	            debug('Creating `tls.Socket`: %o', this.connectOpts);
	            socket = tls.connect(setServernameFromNonIpHost(this.connectOpts));
	        }
	        else {
	            debug('Creating `net.Socket`: %o', this.connectOpts);
	            socket = net.connect(this.connectOpts);
	        }
	        const headers = typeof this.proxyHeaders === 'function'
	            ? this.proxyHeaders()
	            : { ...this.proxyHeaders };
	        const host = net.isIPv6(opts.host) ? `[${opts.host}]` : opts.host;
	        let payload = `CONNECT ${host}:${opts.port} HTTP/1.1\r\n`;
	        // Inject the `Proxy-Authorization` header if necessary.
	        if (proxy.username || proxy.password) {
	            const auth = `${decodeURIComponent(proxy.username)}:${decodeURIComponent(proxy.password)}`;
	            headers['Proxy-Authorization'] = `Basic ${Buffer.from(auth).toString('base64')}`;
	        }
	        headers.Host = `${host}:${opts.port}`;
	        if (!headers['Proxy-Connection']) {
	            headers['Proxy-Connection'] = this.keepAlive
	                ? 'Keep-Alive'
	                : 'close';
	        }
	        for (const name of Object.keys(headers)) {
	            payload += `${name}: ${headers[name]}\r\n`;
	        }
	        const proxyResponsePromise = (0, parse_proxy_response_1.parseProxyResponse)(socket);
	        socket.write(`${payload}\r\n`);
	        const { connect, buffered } = await proxyResponsePromise;
	        req.emit('proxyConnect', connect);
	        this.emit('proxyConnect', connect, req);
	        if (connect.statusCode === 200) {
	            req.once('socket', resume);
	            if (opts.secureEndpoint) {
	                // The proxy is connecting to a TLS server, so upgrade
	                // this socket connection to a TLS connection.
	                debug('Upgrading socket connection to TLS');
	                return tls.connect({
	                    ...omit(setServernameFromNonIpHost(opts), 'host', 'path', 'port'),
	                    socket,
	                });
	            }
	            return socket;
	        }
	        // Some other status code that's not 200... need to re-play the HTTP
	        // header "data" events onto the socket once the HTTP machinery is
	        // attached so that the node core `http` can parse and handle the
	        // error status code.
	        // Close the original socket, and a new "fake" socket is returned
	        // instead, so that the proxy doesn't get the HTTP request
	        // written to it (which may contain `Authorization` headers or other
	        // sensitive data).
	        //
	        // See: https://hackerone.com/reports/541502
	        socket.destroy();
	        const fakeSocket = new net.Socket({ writable: false });
	        fakeSocket.readable = true;
	        // Need to wait for the "socket" event to re-play the "data" events.
	        req.once('socket', (s) => {
	            debug('Replaying proxy buffer for failed request');
	            (0, assert_1.default)(s.listenerCount('data') > 0);
	            // Replay the "buffered" Buffer onto the fake `socket`, since at
	            // this point the HTTP module machinery has been hooked up for
	            // the user.
	            s.push(buffered);
	            s.push(null);
	        });
	        return fakeSocket;
	    }
	}
	HttpsProxyAgent.protocols = ['http', 'https'];
	dist$2.HttpsProxyAgent = HttpsProxyAgent;
	function resume(socket) {
	    socket.resume();
	}
	function omit(obj, ...keys) {
	    const ret = {};
	    let key;
	    for (key in obj) {
	        if (!keys.includes(key)) {
	            ret[key] = obj[key];
	        }
	    }
	    return ret;
	}
	
	return dist$2;
}

var dist = {};

var hasRequiredDist;

function requireDist () {
	if (hasRequiredDist) return dist;
	hasRequiredDist = 1;
	var __createBinding = (dist && dist.__createBinding) || (Object.create ? (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    var desc = Object.getOwnPropertyDescriptor(m, k);
	    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
	      desc = { enumerable: true, get: function() { return m[k]; } };
	    }
	    Object.defineProperty(o, k2, desc);
	}) : (function(o, m, k, k2) {
	    if (k2 === undefined) k2 = k;
	    o[k2] = m[k];
	}));
	var __setModuleDefault = (dist && dist.__setModuleDefault) || (Object.create ? (function(o, v) {
	    Object.defineProperty(o, "default", { enumerable: true, value: v });
	}) : function(o, v) {
	    o["default"] = v;
	});
	var __importStar = (dist && dist.__importStar) || function (mod) {
	    if (mod && mod.__esModule) return mod;
	    var result = {};
	    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
	    __setModuleDefault(result, mod);
	    return result;
	};
	var __importDefault = (dist && dist.__importDefault) || function (mod) {
	    return (mod && mod.__esModule) ? mod : { "default": mod };
	};
	Object.defineProperty(dist, "__esModule", { value: true });
	dist.HttpProxyAgent = void 0;
	const net = __importStar(require$$0$1);
	const tls = __importStar(require$$1);
	const debug_1 = __importDefault(requireSrc());
	const events_1 = require$$0__default$1;
	const agent_base_1 = requireDist$2();
	const url_1 = require$$2;
	const debug = (0, debug_1.default)('http-proxy-agent');
	/**
	 * The `HttpProxyAgent` implements an HTTP Agent subclass that connects
	 * to the specified "HTTP proxy server" in order to proxy HTTP requests.
	 */
	class HttpProxyAgent extends agent_base_1.Agent {
	    constructor(proxy, opts) {
	        super(opts);
	        this.proxy = typeof proxy === 'string' ? new url_1.URL(proxy) : proxy;
	        this.proxyHeaders = opts?.headers ?? {};
	        debug('Creating new HttpProxyAgent instance: %o', this.proxy.href);
	        // Trim off the brackets from IPv6 addresses
	        const host = (this.proxy.hostname || this.proxy.host).replace(/^\[|\]$/g, '');
	        const port = this.proxy.port
	            ? parseInt(this.proxy.port, 10)
	            : this.proxy.protocol === 'https:'
	                ? 443
	                : 80;
	        this.connectOpts = {
	            ...(opts ? omit(opts, 'headers') : null),
	            host,
	            port,
	        };
	    }
	    addRequest(req, opts) {
	        req._header = null;
	        this.setRequestProps(req, opts);
	        // @ts-expect-error `addRequest()` isn't defined in `@types/node`
	        super.addRequest(req, opts);
	    }
	    setRequestProps(req, opts) {
	        const { proxy } = this;
	        const protocol = opts.secureEndpoint ? 'https:' : 'http:';
	        const hostname = req.getHeader('host') || 'localhost';
	        const base = `${protocol}//${hostname}`;
	        const url = new url_1.URL(req.path, base);
	        if (opts.port !== 80) {
	            url.port = String(opts.port);
	        }
	        // Change the `http.ClientRequest` instance's "path" field
	        // to the absolute path of the URL that will be requested.
	        req.path = String(url);
	        // Inject the `Proxy-Authorization` header if necessary.
	        const headers = typeof this.proxyHeaders === 'function'
	            ? this.proxyHeaders()
	            : { ...this.proxyHeaders };
	        if (proxy.username || proxy.password) {
	            const auth = `${decodeURIComponent(proxy.username)}:${decodeURIComponent(proxy.password)}`;
	            headers['Proxy-Authorization'] = `Basic ${Buffer.from(auth).toString('base64')}`;
	        }
	        if (!headers['Proxy-Connection']) {
	            headers['Proxy-Connection'] = this.keepAlive
	                ? 'Keep-Alive'
	                : 'close';
	        }
	        for (const name of Object.keys(headers)) {
	            const value = headers[name];
	            if (value) {
	                req.setHeader(name, value);
	            }
	        }
	    }
	    async connect(req, opts) {
	        req._header = null;
	        if (!req.path.includes('://')) {
	            this.setRequestProps(req, opts);
	        }
	        // At this point, the http ClientRequest's internal `_header` field
	        // might have already been set. If this is the case then we'll need
	        // to re-generate the string since we just changed the `req.path`.
	        let first;
	        let endOfHeaders;
	        debug('Regenerating stored HTTP header string for request');
	        req._implicitHeader();
	        if (req.outputData && req.outputData.length > 0) {
	            debug('Patching connection write() output buffer with updated header');
	            first = req.outputData[0].data;
	            endOfHeaders = first.indexOf('\r\n\r\n') + 4;
	            req.outputData[0].data =
	                req._header + first.substring(endOfHeaders);
	            debug('Output buffer: %o', req.outputData[0].data);
	        }
	        // Create a socket connection to the proxy server.
	        let socket;
	        if (this.proxy.protocol === 'https:') {
	            debug('Creating `tls.Socket`: %o', this.connectOpts);
	            socket = tls.connect(this.connectOpts);
	        }
	        else {
	            debug('Creating `net.Socket`: %o', this.connectOpts);
	            socket = net.connect(this.connectOpts);
	        }
	        // Wait for the socket's `connect` event, so that this `callback()`
	        // function throws instead of the `http` request machinery. This is
	        // important for i.e. `PacProxyAgent` which determines a failed proxy
	        // connection via the `callback()` function throwing.
	        await (0, events_1.once)(socket, 'connect');
	        return socket;
	    }
	}
	HttpProxyAgent.protocols = ['http', 'https'];
	dist.HttpProxyAgent = HttpProxyAgent;
	function omit(obj, ...keys) {
	    const ret = {};
	    let key;
	    for (key in obj) {
	        if (!keys.includes(key)) {
	            ret[key] = obj[key];
	        }
	    }
	    return ret;
	}
	
	return dist;
}

var stateCjs$1 = {};

var hasRequiredStateCjs$1;

function requireStateCjs$1 () {
	if (hasRequiredStateCjs$1) return stateCjs$1;
	hasRequiredStateCjs$1 = 1;
	// Copyright (c) Microsoft Corporation.
	// Licensed under the MIT License.
	Object.defineProperty(stateCjs$1, "__esModule", { value: true });
	stateCjs$1.state = void 0;
	/**
	 * @internal
	 *
	 * Holds the singleton instrumenter, to be shared across CJS and ESM imports.
	 */
	stateCjs$1.state = {
	    instrumenterImplementation: undefined,
	};
	
	return stateCjs$1;
}

var stateCjs = {};

var hasRequiredStateCjs;

function requireStateCjs () {
	if (hasRequiredStateCjs) return stateCjs;
	hasRequiredStateCjs = 1;
	// Copyright (c) Microsoft Corporation.
	// Licensed under the MIT License.
	Object.defineProperty(stateCjs, "__esModule", { value: true });
	stateCjs.state = void 0;
	/**
	 * Holds the singleton operationRequestMap, to be shared across CJS and ESM imports.
	 */
	stateCjs.state = {
	    operationRequestMap: new WeakMap(),
	};
	
	return stateCjs;
}

export { requireDist as a, requireStateCjs$1 as b, requireStateCjs as c, requireSrc as d, requireDist$1 as r };
