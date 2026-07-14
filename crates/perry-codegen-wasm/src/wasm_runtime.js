// Perry WASM Runtime Bridge
// Provides JavaScript runtime functions imported by the WASM module.
// Handles NaN-boxing, string management, handle store, and browser API access.

// NaN-boxing constants (matching perry-runtime/src/value.rs)
const TAG_UNDEFINED = 0x7FFC_0000_0000_0001n;
const TAG_NULL      = 0x7FFC_0000_0000_0002n;
const TAG_FALSE     = 0x7FFC_0000_0000_0003n;
const TAG_TRUE      = 0x7FFC_0000_0000_0004n;
const STRING_TAG    = 0x7FFFn;
const POINTER_TAG   = 0x7FFDn;
const INT32_TAG     = 0x7FFEn;

// f64 <-> u64 conversion via shared buffer
const _convBuf = new ArrayBuffer(8);
const _f64 = new Float64Array(_convBuf);
const _u64 = new BigUint64Array(_convBuf);

function f64ToU64(f) { _f64[0] = f; return _u64[0]; }
function u64ToF64(u) { _u64[0] = u; return _f64[0]; }

// Issue #1035: pad a BigInt arg list to match the WASM function's declared arity.
// Missing trailing slots correspond to TS optional params the caller omitted; the
// WASM ABI expects an i64 (BigInt) per declared slot, so we fill with TAG_UNDEFINED.
// Without this, V8/SpiderMonkey auto-coerce `undefined` via BigInt(undefined) and
// throw "Cannot convert undefined to a BigInt". `prefix` is the count of leading
// args already on the call (e.g. 1 for the receiver, or closure capture count).
function __padBigintArgs(fn, prefix, bigintArgs) {
  const need = Math.max(0, (fn.length | 0) - (prefix | 0));
  while (bigintArgs.length < need) bigintArgs.push(TAG_UNDEFINED);
  return bigintArgs;
}

// NaN-box helpers
function nanboxString(id) {
  return u64ToF64((STRING_TAG << 48n) | BigInt(id));
}
function nanboxPointer(id) {
  return u64ToF64((POINTER_TAG << 48n) | BigInt(id));
}
function isString(val) {
  return (f64ToU64(val) >> 48n) === STRING_TAG;
}
function isPointer(val) {
  return (f64ToU64(val) >> 48n) === POINTER_TAG;
}
function getStringId(val) {
  return Number(f64ToU64(val) & 0xFFFFFFFFn);
}
function getPointerId(val) {
  return Number(f64ToU64(val) & 0xFFFFFFFFn);
}
function isUndefined(val) { return f64ToU64(val) === TAG_UNDEFINED; }
function isNull(val) { return f64ToU64(val) === TAG_NULL; }
function isTrue(val) { return f64ToU64(val) === TAG_TRUE; }
function isFalse(val) { return f64ToU64(val) === TAG_FALSE; }

// String table — maps string_id (index) to JS string
const stringTable = [];

// Handle store — maps handle_id to JS objects/arrays/closures/etc.
const handleStore = new Map();
let nextHandleId = 1;

function allocHandle(obj) {
  const id = nextHandleId++;
  handleStore.set(id, obj);
  return id;
}
function getHandle(val) {
  if (!isPointer(val)) return undefined;
  return handleStore.get(getPointerId(val));
}
function getHandleId(val) {
  return getPointerId(val);
}

// Convert a NaN-boxed f64 value to a JS value
function toJsValue(val) {
  const bits = f64ToU64(val);
  if (bits === TAG_UNDEFINED) return undefined;
  if (bits === TAG_NULL) return null;
  if (bits === TAG_TRUE) return true;
  if (bits === TAG_FALSE) return false;
  const tag = bits >> 48n;
  if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)];
  if (tag === POINTER_TAG) {
    const obj = handleStore.get(Number(bits & 0xFFFFFFFFn));
    if (obj !== undefined) return obj;
  }
  return val; // plain number
}

// Convert a JS value to a NaN-boxed f64
function fromJsValue(v) {
  if (v === undefined) return u64ToF64(TAG_UNDEFINED);
  if (v === null) return u64ToF64(TAG_NULL);
  if (v === true) return u64ToF64(TAG_TRUE);
  if (v === false) return u64ToF64(TAG_FALSE);
  if (typeof v === 'number') return v;
  if (typeof v === 'string') {
    const id = stringTable.length;
    stringTable.push(v);
    return nanboxString(id);
  }
  // Object/Array/Function — store as handle
  const id = allocHandle(v);
  return nanboxPointer(id);
}

// Get string from NaN-boxed value (supports both string tag and pointer to string)
function getString(val) {
  if (isString(val)) return stringTable[getStringId(val)];
  const js = toJsValue(val);
  return String(js);
}

let wasmMemory = null;
let wasmInstance = null;

// Expose NaN-box conversion for FFI bridge (bloom_glue.js etc.)
// These allow external code to convert between Perry's NaN-boxed i64 values and JS values.
globalThis.__perryToJsValue = function(nanboxedF64) { return toJsValue(nanboxedF64); };
globalThis.__perryFromJsValue = function(jsValue) { return fromJsValue(jsValue); };
globalThis.__perryI64ToF64 = function(i64BigInt) { return u64ToF64(i64BigInt); };
globalThis.__perryF64ToI64 = function(f64Val) { return f64ToU64(f64Val); };
// BigInt-native helpers (Issue #133 item 5). Prefer these on Firefox/Safari:
// going through f64 loses NaN-box tag payloads when the Number crosses a JS
// function-call boundary (SpiderMonkey collapses foreign NaNs to the canonical
// 0x7FF8_0000_0000_0000). __perry*Bits helpers skip the f64 round-trip entirely.
// __perryJsValueToBits(v) -> BigInt (raw NaN-box bits, ready to pass to a WASM
// i64 export). __perryBitsToJsValue(bits) -> JS value (handle, string, number, …).
globalThis.__perryJsValueToBits = function(v) { return __jsValueToBits(v); };
globalThis.__perryBitsToJsValue = function(bits) { return __bitsToJsValue(bits); };

// Build the import object for WASM instantiation
function buildImports() {
  return {
    rt: {
      // ===== Core (Phase 0) =====

      string_new: (offset, len) => {
        const bytes = new Uint8Array(wasmMemory.buffer, offset, len);
        stringTable.push(new TextDecoder().decode(bytes));
      },
      console_log: (val) => { console.log(toJsValue(val)); },
      console_warn: (val) => { console.warn(toJsValue(val)); },
      console_error: (val) => { console.error(toJsValue(val)); },

      string_concat: (a, b) => {
        const s = stringTable[getStringId(a)] + stringTable[getStringId(b)];
        stringTable.push(s);
        return nanboxString(stringTable.length - 1);
      },
      js_add: (a, b) => fromJsValue(toJsValue(a) + toJsValue(b)),
      string_eq: (a, b) => stringTable[getStringId(a)] === stringTable[getStringId(b)] ? 1 : 0,
      string_len: (val) => {
        if (isString(val)) return stringTable[getStringId(val)].length;
        // Array length
        const obj = getHandle(val);
        if (Array.isArray(obj)) return obj.length;
        return 0;
      },
      jsvalue_to_string: (val) => {
        stringTable.push(String(toJsValue(val)));
        return nanboxString(stringTable.length - 1);
      },
      is_truthy: (val) => {
        const bits = f64ToU64(val);
        if (bits === TAG_FALSE || bits === TAG_NULL || bits === TAG_UNDEFINED) return 0;
        if (bits === TAG_TRUE) return 1;
        const tag = bits >> 48n;
        if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)].length > 0 ? 1 : 0;
        if (tag === POINTER_TAG) return 1; // objects are truthy
        return (val === 0 || Number.isNaN(val)) ? 0 : 1;
      },
      js_strict_eq: (a, b) => toJsValue(a) === toJsValue(b) ? 1 : 0,

      // Math
      math_floor: (x) => Math.floor(x),
      math_ceil: (x) => Math.ceil(x),
      math_round: (x) => Math.round(x),
      math_abs: (x) => Math.abs(x),
      math_sqrt: (x) => Math.sqrt(x),
      math_pow: (base, exp) => Math.pow(base, exp),
      math_random: () => Math.random(),
      math_log: (x) => Math.log(x),
      date_now: () => Date.now(),
      js_typeof: (val) => {
        const t = typeof toJsValue(val);
        stringTable.push(t);
        return nanboxString(stringTable.length - 1);
      },
      math_min: (a, b) => Math.min(a, b),
      math_max: (a, b) => Math.max(a, b),
      parse_int: (val) => {
        const s = isString(val) ? stringTable[getStringId(val)] : String(toJsValue(val));
        return parseInt(s, 10);
      },
      parse_float: (val) => {
        const s = isString(val) ? stringTable[getStringId(val)] : String(toJsValue(val));
        return parseFloat(s);
      },

      // Phase 0 fixes
      js_mod: (a, b) => a % b,
      is_null_or_undefined: (val) => {
        const bits = f64ToU64(val);
        return (bits === TAG_NULL || bits === TAG_UNDEFINED) ? 1 : 0;
      },

      // ===== Phase 1: Objects =====

      object_new: () => nanboxPointer(allocHandle({})),

      // object_set(handle, key_str, value) -> handle (for chaining)
      object_set: (handle, key, value) => {
        const obj = getHandle(handle);
        if (obj) obj[getString(key)] = toJsValue(value);
        return handle;
      },
      object_get: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(obj[getString(key)]);
      },
      object_get_dynamic: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        const k = toJsValue(key);
        return fromJsValue(obj[k]);
      },
      object_set_dynamic: (handle, key, value) => {
        const obj = getHandle(handle);
        if (obj) obj[toJsValue(key)] = toJsValue(value);
      },
      object_delete: (handle, key) => {
        const obj = getHandle(handle);
        if (obj) delete obj[getString(key)];
      },
      object_delete_dynamic: (handle, key) => {
        const obj = getHandle(handle);
        if (obj) delete obj[toJsValue(key)];
      },
      object_keys: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.keys(obj).filter(k => k !== "__class__") : []));
      },
      object_values: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.keys(obj).filter(k => k !== "__class__").map(k => obj[k]) : []));
      },
      object_entries: (handle) => {
        const obj = getHandle(handle);
        return nanboxPointer(allocHandle(obj ? Object.entries(obj).filter(([k]) => k !== "__class__") : []));
      },
      object_has_property: (handle, key) => {
        const obj = getHandle(handle);
        if (!obj) return 0;
        const k = toJsValue(key);
        return (k in obj) ? 1 : 0;
      },
      // object_assign(target, source) -> target handle
      object_assign: (target, source) => {
        const rawTarget = toJsValue(target);
        if (rawTarget == null) throw new TypeError('Cannot convert undefined or null to object');
        const t = Object(rawTarget);
        const s = toJsValue(source);
        if (s != null) Object.assign(t, s);
        return isPointer(target) ? target : fromJsValue(t);
      },

      // ===== Phase 1: Arrays =====

      array_new: () => nanboxPointer(allocHandle([])),

      // array_push(handle, value) -> handle (for chaining)
      array_push: (handle, value) => {
        const arr = getHandle(handle);
        if (arr) arr.push(toJsValue(value));
        return handle;
      },
      array_pop: (handle) => {
        const arr = getHandle(handle);
        if (!arr || arr.length === 0) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(arr.pop());
      },
      array_get: (handle, index) => {
        const arr = getHandle(handle);
        if (!arr) return u64ToF64(TAG_UNDEFINED);
        const i = typeof index === 'number' ? index : toJsValue(index);
        return fromJsValue(arr[i]);
      },
      array_set: (handle, index, value) => {
        const arr = getHandle(handle);
        if (arr) arr[typeof index === 'number' ? index : toJsValue(index)] = toJsValue(value);
      },
      array_length: (handle) => {
        const arr = getHandle(handle);
        return arr ? arr.length : 0;
      },
      array_slice: (handle, start, end) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxPointer(allocHandle([]));
        const s = typeof start === 'number' ? start : 0;
        const e = isUndefined(end) ? undefined : (typeof end === 'number' ? end : toJsValue(end));
        return nanboxPointer(allocHandle(arr.slice(s, e)));
      },
      array_splice: (handle, start, deleteCount) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxPointer(allocHandle([]));
        const s = typeof start === 'number' ? start : 0;
        const dc = isUndefined(deleteCount) ? arr.length - s : deleteCount;
        return nanboxPointer(allocHandle(arr.splice(s, dc)));
      },
      array_shift: (handle) => {
        const arr = getHandle(handle);
        if (!arr || arr.length === 0) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(arr.shift());
      },
      array_unshift: (handle, value) => {
        const arr = getHandle(handle);
        if (arr) arr.unshift(toJsValue(value));
      },
      array_join: (handle, separator) => {
        const arr = getHandle(handle);
        if (!arr) return nanboxString(allocHandle(''));
        const sep = getString(separator);
        const result = arr.map(v => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)).join(sep);
        stringTable.push(result);
        return nanboxString(stringTable.length - 1);
      },
      array_index_of: (handle, value) => {
        const arr = getHandle(handle);
        if (!arr) return -1;
        const v = toJsValue(value);
        return arr.indexOf(v);
      },
      array_includes: (handle, value) => {
        const arr = getHandle(handle);
        if (!arr) return 0;
        return arr.includes(toJsValue(value)) ? 1 : 0;
      },
      array_concat: (h1, h2) => {
        const a1 = getHandle(h1) || [];
        const a2 = getHandle(h2) || [];
        return nanboxPointer(allocHandle(a1.concat(a2)));
      },
      array_reverse: (handle) => {
        const arr = getHandle(handle);
        if (arr) arr.reverse();
        return handle;
      },
      array_flat: (handle) => {
        const arr = getHandle(handle);
        return nanboxPointer(allocHandle(arr ? arr.flat() : []));
      },
      array_is_array: (val) => {
        if (!isPointer(val)) return 0;
        const obj = getHandle(val);
        return Array.isArray(obj) ? 1 : 0;
      },
      array_from: (val) => {
        const v = toJsValue(val);
        return nanboxPointer(allocHandle(Array.from(v)));
      },
      array_push_spread: (target, source) => {
        const t = getHandle(target);
        const s = getHandle(source);
        if (t && s) t.push(...s);
        return target;
      },

      // ===== Phase 1: String methods =====

      string_charAt: (str, idx) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.charAt(idx));
        return nanboxString(stringTable.length - 1);
      },
      string_substring: (str, start, end) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.substring(start, end));
        return nanboxString(stringTable.length - 1);
      },
      string_indexOf: (str, search) => {
        return stringTable[getStringId(str)].indexOf(stringTable[getStringId(search)]);
      },
      string_slice: (str, start, end) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.slice(start, end));
        return nanboxString(stringTable.length - 1);
      },
      string_toLowerCase: (str) => {
        stringTable.push(stringTable[getStringId(str)].toLowerCase());
        return nanboxString(stringTable.length - 1);
      },
      string_toUpperCase: (str) => {
        stringTable.push(stringTable[getStringId(str)].toUpperCase());
        return nanboxString(stringTable.length - 1);
      },
      string_trim: (str) => {
        stringTable.push(stringTable[getStringId(str)].trim());
        return nanboxString(stringTable.length - 1);
      },
      string_includes: (str, search) => {
        return stringTable[getStringId(str)].includes(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_startsWith: (str, search) => {
        return stringTable[getStringId(str)].startsWith(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_endsWith: (str, search) => {
        return stringTable[getStringId(str)].endsWith(stringTable[getStringId(search)]) ? 1 : 0;
      },
      string_replace: (str, pattern, replacement) => {
        const s = stringTable[getStringId(str)];
        // pattern might be a regex handle or a string
        let p, r;
        if (isPointer(pattern)) {
          p = getHandle(pattern); // RegExp object
        } else {
          p = getString(pattern);
        }
        r = getString(replacement);
        stringTable.push(s.replace(p, r));
        return nanboxString(stringTable.length - 1);
      },
      string_split: (str, delim) => {
        const s = stringTable[getStringId(str)];
        const d = getString(delim);
        return nanboxPointer(allocHandle(s.split(d)));
      },
      string_fromCharCode: (code) => {
        stringTable.push(String.fromCharCode(code));
        return nanboxString(stringTable.length - 1);
      },
      string_padStart: (str, len, fill) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.padStart(len, getString(fill)));
        return nanboxString(stringTable.length - 1);
      },
      string_padEnd: (str, len, fill) => {
        const s = stringTable[getStringId(str)];
        stringTable.push(s.padEnd(len, getString(fill)));
        return nanboxString(stringTable.length - 1);
      },
      string_repeat: (str, count) => {
        stringTable.push(stringTable[getStringId(str)].repeat(count));
        return nanboxString(stringTable.length - 1);
      },
      string_match: (str, regex) => {
        const s = stringTable[getStringId(str)];
        const re = isPointer(regex) ? getHandle(regex) : new RegExp(getString(regex));
        const result = s.match(re);
        if (!result) return u64ToF64(TAG_NULL);
        return nanboxPointer(allocHandle(Array.from(result)));
      },
      math_log2: (x) => Math.log2(x),
      math_log10: (x) => Math.log10(x),

      // ===== Phase 2: Closures =====

      // closure_new(func_table_idx, capture_count) -> handle
      closure_new: (funcIdx, captureCount) => {
        const closure = { funcIdx: funcIdx, captures: new Array(captureCount | 0) };
        return nanboxPointer(allocHandle(closure));
      },
      // closure_set_capture(handle, idx, value) -> handle (chaining)
      // value is NaN-boxed f64 — convert to BigInt for WASM i64 ABI
      closure_set_capture: (handle, idx, value) => {
        const c = getHandle(handle);
        if (c) c.captures[idx | 0] = f64ToU64(value); // store as BigInt for i64 WASM calls
        return handle;
      },
      // closure_call_N(handle, args...) -> result
      // WASM functions use i64 (BigInt) params/returns. Captures are already BigInt.
      // Args (a0, a1, ...) are NaN-boxed f64 from the old rt: import ABI — convert via f64ToU64.
      // Return is BigInt from WASM — convert back to f64 via u64ToF64.
      closure_call_0: (handle) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        // Pad for omitted TS optional params (Issue #1035).
        const padded = __padBigintArgs(fn, c.captures.length, []);
        return u64ToF64(fn(...c.captures, ...padded));
      },
      closure_call_1: (handle, a0) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        const padded = __padBigintArgs(fn, c.captures.length + 1, [f64ToU64(a0)]);
        return u64ToF64(fn(...c.captures, ...padded));
      },
      closure_call_2: (handle, a0, a1) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        const padded = __padBigintArgs(fn, c.captures.length + 2, [f64ToU64(a0), f64ToU64(a1)]);
        return u64ToF64(fn(...c.captures, ...padded));
      },
      closure_call_3: (handle, a0, a1, a2) => {
        const c = getHandle(handle);
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        const padded = __padBigintArgs(fn, c.captures.length + 3, [f64ToU64(a0), f64ToU64(a1), f64ToU64(a2)]);
        return u64ToF64(fn(...c.captures, ...padded));
      },
      // closure_call_spread(handle, args_array_handle) -> result
      closure_call_spread: (handle, argsHandle) => {
        const c = getHandle(handle);
        const args = getHandle(argsHandle) || [];
        if (!c || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(c.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        const padded = __padBigintArgs(fn, c.captures.length, args.map(v => f64ToU64(fromJsValue(v))));
        return u64ToF64(fn(...c.captures, ...padded));
      },

      // ===== Phase 2: Array higher-order methods =====

      // Helper: call a closure/wasm function with an element
      // The closure is a handle, the callback takes (element, index, array)
      array_map: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return nanboxPointer(allocHandle([]));
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return nanboxPointer(allocHandle([]));
        const result = arr.map((v, i) => {
          const r = fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i));
          return __bitsToJsValue(r);
        });
        return nanboxPointer(allocHandle(result));
      },
      array_filter: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return nanboxPointer(allocHandle([]));
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return nanboxPointer(allocHandle([]));
        const result = arr.filter((v, i) => {
          const r = fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i));
          const rv = __bitsToJsValue(r);
          if (rv === true) return true;
          if (rv === false || rv === null || rv === undefined) return false;
          return !!rv;
        });
        return nanboxPointer(allocHandle(result));
      },
      array_forEach: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return;
        arr.forEach((v, i) => fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)));
      },
      array_reduce: (handle, cbHandle, initial) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        let acc = isUndefined(initial) ? __jsValueToBits(arr[0]) : f64ToU64(initial);
        const startIdx = isUndefined(initial) ? 1 : 0;
        for (let i = startIdx; i < arr.length; i++) {
          acc = fn(...cb.captures, acc, __jsValueToBits(arr[i]), __jsValueToBits(i));
        }
        return u64ToF64(acc);
      },
      array_find: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return u64ToF64(TAG_UNDEFINED);
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return u64ToF64(TAG_UNDEFINED);
        for (let i = 0; i < arr.length; i++) {
          const vBits = __jsValueToBits(arr[i]);
          const r = fn(...cb.captures, vBits, __jsValueToBits(i));
          if (__bitsToJsValue(r)) return fromJsValue(arr[i]);
        }
        return u64ToF64(TAG_UNDEFINED);
      },
      array_find_index: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return -1;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return -1;
        for (let i = 0; i < arr.length; i++) {
          const r = fn(...cb.captures, __jsValueToBits(arr[i]), __jsValueToBits(i));
          if (__bitsToJsValue(r)) return i;
        }
        return -1;
      },
      array_sort: (handle, cbHandle) => {
        const arr = getHandle(handle);
        if (!arr) return handle;
        if (isUndefined(cbHandle) || isNull(cbHandle)) {
          arr.sort();
          return handle;
        }
        const cb = getHandle(cbHandle);
        if (!cb || !wasmInstance) { arr.sort(); return handle; }
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) { arr.sort(); return handle; }
        arr.sort((a, b) => { const r = fn(...cb.captures, __jsValueToBits(a), __jsValueToBits(b)); return __bitsToJsValue(r); });
        return handle;
      },
      array_some: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return 0;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return 0;
        return arr.some((v, i) => __bitsToJsValue(fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)))) ? 1 : 0;
      },
      array_every: (handle, cbHandle) => {
        const arr = getHandle(handle);
        const cb = getHandle(cbHandle);
        if (!arr || !cb || !wasmInstance) return 0;
        const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
        if (!fn) return 0;
        return arr.every((v, i) => __bitsToJsValue(fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)))) ? 1 : 0;
      },

      // ===== Phase 3: Classes =====

      // Class registry: class_name -> { methods: Map, statics: Map, parent: string|null }
      // class_new(class_name_str, field_count) -> handle
      class_new: (classNameVal, fieldCount) => {
        const obj = {};
        obj.__class__ = getString(classNameVal);
        return nanboxPointer(allocHandle(obj));
      },
      // class_set_method(class_id_str, method_name_str, func_table_idx)
      class_set_method: (classId, methodName, funcIdx) => {
        const cls = getString(classId);
        if (!classMethodTable[cls]) classMethodTable[cls] = {};
        classMethodTable[cls][getString(methodName)] = funcIdx | 0;
      },
      // class_call_method(handle, method_name_id, args_array_handle) -> result
      // method_name_id is a plain f64 string table index (not NaN-boxed).
      // WASM functions use i64 (BigInt) params/returns.
      class_call_method: (handle, methodNameId, argsHandle) => {
        const obj = getHandle(handle);
        const mname = stringTable[methodNameId | 0];
        // Try class method dispatch first
        if (obj && obj.__class__) {
          let cls = obj.__class__;
          while (cls) {
            const methods = classMethodTable[cls];
            if (methods && mname in methods) {
              const fn = wasmInstance?.exports.__indirect_function_table?.get(methods[mname]);
              if (fn) {
                const args = getHandle(argsHandle) || [];
                // Pad for omitted TS optional params (Issue #1035).
                const padded = __padBigintArgs(fn, 1, args.map(v => f64ToU64(fromJsValue(v))));
                return u64ToF64(fn(f64ToU64(handle), ...padded));
              }
            }
            cls = classParentTable[cls] || null;
          }
        }
        // Fallback: UI widget method dispatch
        // Map common method names to perry_ui_* bridge functions
        const uiMethodMap = {
          addChild: "perry_ui_widget_add_child", removeAllChildren: "perry_ui_widget_remove_all_children",
          setBackground: "perry_ui_set_background", setForeground: "perry_ui_set_foreground",
          setFontSize: "perry_ui_set_font_size", setFontWeight: "perry_ui_set_font_weight",
          setFontFamily: "perry_ui_set_font_family", setPadding: "perry_ui_set_padding",
          setFrame: "perry_ui_set_frame", setCornerRadius: "perry_ui_set_corner_radius",
          setBorder: "perry_ui_set_border", setOpacity: "perry_ui_set_opacity",
          setEnabled: "perry_ui_set_enabled", setTooltip: "perry_ui_set_tooltip",
          setControlSize: "perry_ui_set_control_size",
          animateOpacity: "perry_ui_animate_opacity", animatePosition: "perry_ui_animate_position",
          setOnClick: "perry_ui_set_on_click", setOnHover: "perry_ui_set_on_hover",
          setOnDoubleClick: "perry_ui_set_on_double_click",
          // State methods
          get: "perry_ui_state_get", set: "perry_ui_state_set",
          create: "perry_ui_state_create",
          bindText: "perry_ui_state_bind_text", bindTextNumeric: "perry_ui_state_bind_text_numeric",
          bindSlider: "perry_ui_state_bind_slider", bindToggle: "perry_ui_state_bind_toggle",
          bindVisibility: "perry_ui_state_bind_visibility", bindForEach: "perry_ui_state_bind_foreach",
          onChange: "perry_ui_state_on_change",
          // Text/Button ops
          setString: "perry_ui_text_set_string", setSelectable: "perry_ui_text_set_selectable",
          setBordered: "perry_ui_button_set_bordered", setTitle: "perry_ui_button_set_title",
          setTextColor: "perry_ui_button_set_text_color", setImage: "perry_ui_button_set_image",
          // Canvas
          fillRect: "perry_ui_canvas_fill_rect", strokeRect: "perry_ui_canvas_stroke_rect",
          clearRect: "perry_ui_canvas_clear_rect", setFillColor: "perry_ui_canvas_set_fill_color",
          setStrokeColor: "perry_ui_canvas_set_stroke_color", beginPath: "perry_ui_canvas_begin_path",
          moveTo: "perry_ui_canvas_move_to", lineTo: "perry_ui_canvas_line_to",
          arc: "perry_ui_canvas_arc", closePath: "perry_ui_canvas_close_path",
          fill: "perry_ui_canvas_fill", stroke: "perry_ui_canvas_stroke",
          setLineWidth: "perry_ui_canvas_set_line_width", fillText: "perry_ui_canvas_fill_text",
          setFont: "perry_ui_canvas_set_font", drawImage: "perry_ui_canvas_draw_image",
          // ScrollView
          setChild: "perry_ui_scrollview_set_child",
          // TextField
          focus: "perry_ui_textfield_focus",
          // Widget sizing
          setWidth: "perry_ui_widget_set_width", setHeight: "perry_ui_widget_set_height",
          matchParentWidth: "perry_ui_widget_match_parent_width",
          matchParentHeight: "perry_ui_widget_match_parent_height",
          setHidden: "perry_ui_set_widget_hidden",
          setEdgeInsets: "perry_ui_widget_set_edge_insets",
          setShadow: "perry_ui_widget_set_shadow",
          // App
          run: "perry_ui_app_run", setBody: "perry_ui_app_set_body",
        };
        const uiFnName = uiMethodMap[mname];
        if (uiFnName) {
          const fn = __perryUiDispatch[uiFnName];
          if (fn) {
            const args = getHandle(argsHandle) || [];
            // First arg to the UI function is the handle/object, rest are the call args
            const objVal = toJsValue(handle);
            // Check if any args are closures (POINTER_TAG) and keep them raw for callWasmClosure
            const closureMethods = new Set(["setOnClick","setOnHover","setOnDoubleClick","onChange","bindForEach","set"]);
            if (closureMethods.has(mname)) {
              const jsArgs = args.map(v => {
                const bits = f64ToU64(v);
                if ((bits >> 48n) === POINTER_TAG) return v;
                return toJsValue(v);
              });
              return fromJsValue(fn(objVal, ...jsArgs));
            }
            const jsArgs = args.map(v => toJsValue(v));
            return fromJsValue(fn(objVal, ...jsArgs));
          }
        }
        return u64ToF64(TAG_UNDEFINED);
      },
      class_get_field: (handle, name) => {
        const obj = getHandle(handle);
        if (!obj) return u64ToF64(TAG_UNDEFINED);
        const fname = getString(name);
        // Check for compiled getter method — WASM uses i64 (BigInt)
        if (obj.__class__) {
          let cls = obj.__class__;
          while (cls) {
            const methods = classMethodTable[cls];
            if (methods && ('__get_' + fname) in methods) {
              const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__get_' + fname]);
              if (fn) return u64ToF64(fn(f64ToU64(handle)));
            }
            cls = classParentTable[cls] || null;
          }
        }
        return fromJsValue(obj[fname]);
      },
      class_set_field: (handle, name, value) => {
        const obj = getHandle(handle);
        if (!obj) return;
        const fname = getString(name);
        // Check for compiled setter method — WASM uses i64 (BigInt)
        if (obj.__class__) {
          let cls = obj.__class__;
          while (cls) {
            const methods = classMethodTable[cls];
            if (methods && ('__set_' + fname) in methods) {
              const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__set_' + fname]);
              if (fn) { fn(f64ToU64(handle), f64ToU64(value)); return; }
            }
            cls = classParentTable[cls] || null;
          }
        }
        obj[fname] = toJsValue(value);
      },
      class_set_static: (classId, name, value) => {
        const cls = getString(classId);
        if (!classStaticTable[cls]) classStaticTable[cls] = {};
        classStaticTable[cls][getString(name)] = toJsValue(value);
      },
      class_get_static: (classId, name) => {
        const cls = getString(classId);
        const statics = classStaticTable[cls];
        if (!statics) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(statics[getString(name)]);
      },
      class_instanceof: (handle, classId) => {
        const obj = getHandle(handle);
        if (!obj) return 0;
        let cls = obj.__class__;
        const target = getString(classId);
        while (cls) {
          if (cls === target) return 1;
          cls = classParentTable[cls] || null;
        }
        return 0;
      },

      // ===== Phase 4: JSON =====

      json_parse: (str) => {
        try {
          const s = getString(str);
          return fromJsValue(JSON.parse(s));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      json_stringify: (val) => {
        const js = toJsValue(val);
        stringTable.push(JSON.stringify(js));
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Map =====

      map_new: () => nanboxPointer(allocHandle(new Map())),
      map_set: (handle, key, value) => {
        const m = getHandle(handle);
        if (m) m.set(toJsValue(key), toJsValue(value));
      },
      map_get: (handle, key) => {
        const m = getHandle(handle);
        if (!m) return u64ToF64(TAG_UNDEFINED);
        return fromJsValue(m.get(toJsValue(key)));
      },
      map_has: (handle, key) => {
        const m = getHandle(handle);
        return m?.has(toJsValue(key)) ? 1 : 0;
      },
      map_delete: (handle, key) => {
        const m = getHandle(handle);
        if (m) m.delete(toJsValue(key));
      },
      map_size: (handle) => {
        const m = getHandle(handle);
        return m ? m.size : 0;
      },
      map_clear: (handle) => {
        const m = getHandle(handle);
        if (m) m.clear();
      },
      map_entries: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.entries()] : []));
      },
      map_keys: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.keys()] : []));
      },
      map_values: (handle) => {
        const m = getHandle(handle);
        return nanboxPointer(allocHandle(m ? [...m.values()] : []));
      },

      // ===== Phase 4: Set =====

      set_new: () => nanboxPointer(allocHandle(new Set())),
      set_new_from_array: (arrHandle) => {
        const arr = getHandle(arrHandle) || toJsValue(arrHandle);
        return nanboxPointer(allocHandle(new Set(Array.isArray(arr) ? arr : [])));
      },
      set_add: (handle, value) => {
        const s = getHandle(handle);
        if (s) s.add(toJsValue(value));
      },
      set_has: (handle, value) => {
        const s = getHandle(handle);
        return s?.has(toJsValue(value)) ? 1 : 0;
      },
      set_delete: (handle, value) => {
        const s = getHandle(handle);
        if (s) s.delete(toJsValue(value));
      },
      set_size: (handle) => {
        const s = getHandle(handle);
        return s ? s.size : 0;
      },
      set_clear: (handle) => {
        const s = getHandle(handle);
        if (s) s.clear();
      },
      set_values: (handle) => {
        const s = getHandle(handle);
        return nanboxPointer(allocHandle(s ? [...s.values()] : []));
      },

      // ===== Phase 4: Date =====

      date_new_val: (arg) => {
        const d = isUndefined(arg) ? new Date() : new Date(toJsValue(arg));
        return nanboxPointer(allocHandle(d));
      },
      date_get_time: (handle) => {
        const d = getHandle(handle);
        return d instanceof Date ? d.getTime() : 0;
      },
      date_to_iso_string: (handle) => {
        const d = getHandle(handle);
        if (!(d instanceof Date)) return u64ToF64(TAG_UNDEFINED);
        stringTable.push(d.toISOString());
        return nanboxString(stringTable.length - 1);
      },
      date_get_full_year: (h) => { const d = getHandle(h); return d instanceof Date ? d.getFullYear() : 0; },
      date_get_month: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMonth() : 0; },
      date_get_date: (h) => { const d = getHandle(h); return d instanceof Date ? d.getDate() : 0; },
      date_get_day: (h) => { const d = getHandle(h); return d instanceof Date ? d.getDay() : 0; },
      date_get_hours: (h) => { const d = getHandle(h); return d instanceof Date ? d.getHours() : 0; },
      date_get_minutes: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMinutes() : 0; },
      date_get_seconds: (h) => { const d = getHandle(h); return d instanceof Date ? d.getSeconds() : 0; },
      date_get_milliseconds: (h) => { const d = getHandle(h); return d instanceof Date ? d.getMilliseconds() : 0; },

      // ===== Phase 4: Error =====

      error_new: (msg) => {
        const message = isUndefined(msg) ? undefined : getString(msg);
        return nanboxPointer(allocHandle(new Error(message)));
      },
      error_message: (handle) => {
        const e = getHandle(handle);
        const msg = e instanceof Error ? e.message : '';
        stringTable.push(msg);
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: RegExp =====

      regexp_new: (pattern, flags) => {
        try {
          const p = getString(pattern);
          const f = getString(flags);
          return nanboxPointer(allocHandle(new RegExp(p, f)));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      regexp_test: (regex, str) => {
        const re = getHandle(regex);
        if (!(re instanceof RegExp)) return 0;
        return re.test(getString(str)) ? 1 : 0;
      },

      // ===== Phase 4: Globals =====

      number_coerce: (val) => {
        const v = toJsValue(val);
        return Number(v);
      },
      is_nan: (val) => Number.isNaN(val) ? 1 : 0,
      is_finite: (val) => Number.isFinite(val) ? 1 : 0,

      // ===== Phase 5: Misc =====

      console_log_multi: (argsHandle) => {
        const args = getHandle(argsHandle);
        if (args) console.log(...args.map(toJsValue));
      },

      // ===== Phase 1 Addition: Class inheritance =====

      class_set_parent: (childId, parentId) => {
        const child = getString(childId);
        const parent = getString(parentId);
        classParentTable[child] = parent;
      },

      // ===== Phase 3: Try/Catch =====

      try_start: () => { tryDepth++; },
      try_end: () => { if (tryDepth > 0) tryDepth--; },
      throw_value: (val) => { currentException = val; },
      has_exception: () => currentException !== null ? 1 : 0,
      get_exception: () => {
        const e = currentException;
        currentException = null;
        return e !== null ? e : u64ToF64(TAG_UNDEFINED);
      },

      // ===== Phase 4: URL =====

      url_parse: (urlStr) => {
        try {
          return nanboxPointer(allocHandle(new URL(getString(urlStr))));
        } catch (e) {
          if (tryDepth > 0) { currentException = fromJsValue(e); }
          return u64ToF64(TAG_UNDEFINED);
        }
      },
      url_get_href: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.href); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_pathname: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.pathname); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_hostname: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.hostname); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_port: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.port); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_search: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.search); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_hash: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.hash); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_origin: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.origin); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_protocol: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) { stringTable.push(u.protocol); return nanboxString(stringTable.length - 1); }
        return u64ToF64(TAG_UNDEFINED);
      },
      url_get_search_params: (handle) => {
        const u = getHandle(handle);
        if (u instanceof URL) return nanboxPointer(allocHandle(u.searchParams));
        return u64ToF64(TAG_UNDEFINED);
      },
      searchparams_get: (handle, key) => {
        const sp = getHandle(handle);
        if (!sp || typeof sp.get !== 'function') return u64ToF64(TAG_NULL);
        const v = sp.get(getString(key));
        if (v === null) return u64ToF64(TAG_NULL);
        stringTable.push(v);
        return nanboxString(stringTable.length - 1);
      },
      searchparams_has: (handle, key) => {
        const sp = getHandle(handle);
        return (sp && typeof sp.has === 'function' && sp.has(getString(key))) ? 1 : 0;
      },
      searchparams_set: (handle, key, val) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.set === 'function') sp.set(getString(key), getString(val));
      },
      searchparams_append: (handle, key, val) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.append === 'function') sp.append(getString(key), getString(val));
      },
      searchparams_delete: (handle, key) => {
        const sp = getHandle(handle);
        if (sp && typeof sp.delete === 'function') sp.delete(getString(key));
      },
      searchparams_to_string: (handle) => {
        const sp = getHandle(handle);
        stringTable.push(sp ? sp.toString() : '');
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Crypto =====

      crypto_random_uuid: () => {
        stringTable.push(crypto.randomUUID());
        return nanboxString(stringTable.length - 1);
      },
      crypto_random_bytes: (n) => {
        return nanboxPointer(allocHandle(crypto.getRandomValues(new Uint8Array(n))));
      },

      // ===== Phase 4: Path =====

      path_join: (a, b) => {
        const sa = getString(a), sb = getString(b);
        const joined = (sa + '/' + sb).replace(/\/+/g, '/');
        stringTable.push(joined);
        return nanboxString(stringTable.length - 1);
      },
      path_dirname: (str) => {
        const s = getString(str);
        const idx = s.lastIndexOf('/');
        stringTable.push(idx >= 0 ? (s.substring(0, idx) || '/') : '.');
        return nanboxString(stringTable.length - 1);
      },
      path_basename: (str) => {
        const s = getString(str);
        const idx = s.lastIndexOf('/');
        stringTable.push(idx >= 0 ? s.substring(idx + 1) : s);
        return nanboxString(stringTable.length - 1);
      },
      path_extname: (str) => {
        const s = getString(str);
        const base = s.substring(s.lastIndexOf('/') + 1);
        const idx = base.lastIndexOf('.');
        stringTable.push(idx > 0 ? base.substring(idx) : '');
        return nanboxString(stringTable.length - 1);
      },
      path_resolve: (str) => {
        stringTable.push(getString(str));
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 4: Process/OS =====

      os_platform: () => {
        stringTable.push('wasm');
        return nanboxString(stringTable.length - 1);
      },
      process_argv: () => nanboxPointer(allocHandle([])),
      process_cwd: () => {
        stringTable.push('/');
        return nanboxString(stringTable.length - 1);
      },

      // ===== Phase 6: Buffer/Uint8Array =====

      buffer_alloc: (size) => nanboxPointer(allocHandle(new Uint8Array(size))),
      buffer_from_string: (str, encoding) => {
        const s = getString(str);
        return nanboxPointer(allocHandle(new TextEncoder().encode(s)));
      },
      buffer_to_string: (handle, encoding) => {
        const buf = getHandle(handle);
        if (!buf) { stringTable.push(''); return nanboxString(stringTable.length - 1); }
        stringTable.push(new TextDecoder().decode(buf));
        return nanboxString(stringTable.length - 1);
      },
      buffer_get: (handle, idx) => {
        const buf = getHandle(handle);
        return buf ? buf[idx] : 0;
      },
      buffer_set: (handle, idx, val) => {
        const buf = getHandle(handle);
        if (buf) buf[idx] = val;
      },
      buffer_length: (handle) => {
        const buf = getHandle(handle);
        return buf ? buf.length : 0;
      },
      buffer_slice: (handle, start, end) => {
        const buf = getHandle(handle);
        if (!buf) return nanboxPointer(allocHandle(new Uint8Array(0)));
        const e = isUndefined(end) ? undefined : end;
        return nanboxPointer(allocHandle(buf.slice(start, e)));
      },
      buffer_concat: (arrHandle) => {
        const arr = getHandle(arrHandle);
        if (!arr || !Array.isArray(arr)) return nanboxPointer(allocHandle(new Uint8Array(0)));
        const bufs = arr.map(v => {
          const h = typeof v === 'number' ? getHandle(v) : v;
          return h instanceof Uint8Array ? h : new Uint8Array(0);
        });
        const total = bufs.reduce((s, b) => s + b.length, 0);
        const result = new Uint8Array(total);
        let offset = 0;
        bufs.forEach(b => { result.set(b, offset); offset += b.length; });
        return nanboxPointer(allocHandle(result));
      },
      uint8array_new: (size) => nanboxPointer(allocHandle(new Uint8Array(size))),
      uint8array_from: (val) => {
        const v = toJsValue(val);
        return nanboxPointer(allocHandle(Uint8Array.from(Array.isArray(v) ? v : [])));
      },
      uint8array_length: (handle) => {
        const buf = getHandle(handle);
        return buf ? buf.length : 0;
      },
      uint8array_get: (handle, idx) => {
        const buf = getHandle(handle);
        return buf ? buf[idx] : 0;
      },
      uint8array_set: (handle, idx, val) => {
        const buf = getHandle(handle);
        if (buf) buf[idx] = val;
      },

      // ===== Timers =====

      set_timeout: (closureHandle, delay) => {
        const cb = getHandle(closureHandle);
        if (!cb || !wasmInstance) return 0;
        const id = setTimeout(() => {
          const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
          if (fn) fn(...cb.captures); // captures are already BigInt
        }, delay);
        return id;
      },
      set_interval: (closureHandle, delay) => {
        const cb = getHandle(closureHandle);
        if (!cb || !wasmInstance) return 0;
        const id = setInterval(() => {
          const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
          if (fn) fn(...cb.captures); // captures are already BigInt
        }, delay);
        return id;
      },
      clear_timeout: (id) => { clearTimeout(id); },
      clear_interval: (id) => { clearInterval(id); },

      // ===== Response properties =====

      response_status: (handle) => {
        const r = getHandle(handle);
        return (r && typeof r.status === 'number') ? r.status : 0;
      },
      response_ok: (handle) => {
        const r = getHandle(handle);
        return (r && r.ok) ? 1 : 0;
      },
      response_headers_get: (handle, name) => {
        const r = getHandle(handle);
        if (!r || !r.headers) return u64ToF64(TAG_NULL);
        const v = r.headers.get(getString(name));
        if (v === null) return u64ToF64(TAG_NULL);
        stringTable.push(v);
        return nanboxString(stringTable.length - 1);
      },
      response_url: (handle) => {
        const r = getHandle(handle);
        if (!r || typeof r.url !== 'string') return u64ToF64(TAG_UNDEFINED);
        stringTable.push(r.url);
        return nanboxString(stringTable.length - 1);
      },

      // ===== Buffer extras =====

      buffer_copy: (source, target, targetStart, sourceStart, sourceEnd) => {
        const src = getHandle(source);
        const tgt = getHandle(target);
        if (!src || !tgt) return 0;
        const ts = isUndefined(targetStart) ? 0 : targetStart;
        const ss = isUndefined(sourceStart) ? 0 : sourceStart;
        const se = isUndefined(sourceEnd) ? src.length : sourceEnd;
        let copied = 0;
        for (let i = ss; i < se && (ts + copied) < tgt.length; i++) {
          tgt[ts + copied] = src[i];
          copied++;
        }
        return copied;
      },
      buffer_write: (handle, str, offset, encoding) => {
        const buf = getHandle(handle);
        if (!buf) return 0;
        const s = getString(str);
        const encoded = new TextEncoder().encode(s);
        const off = isUndefined(offset) ? 0 : offset;
        let written = 0;
        for (let i = 0; i < encoded.length && (off + i) < buf.length; i++) {
          buf[off + i] = encoded[i];
          written++;
        }
        return written;
      },
      buffer_equals: (handle, other) => {
        const a = getHandle(handle);
        const b = getHandle(other);
        if (!a || !b) return 0;
        if (a.length !== b.length) return 0;
        for (let i = 0; i < a.length; i++) {
          if (a[i] !== b[i]) return 0;
        }
        return 1;
      },
      buffer_is_buffer: (val) => {
        if (!isPointer(val)) return 0;
        const obj = getHandle(val);
        return (obj instanceof Uint8Array || (obj && obj.constructor && obj.constructor.name === 'Buffer')) ? 1 : 0;
      },
      buffer_byte_length: (val) => {
        if (isString(val)) return new TextEncoder().encode(getString(val)).length;
        if (isPointer(val)) {
          const obj = getHandle(val);
          if (obj instanceof Uint8Array) return obj.length;
        }
        return 0;
      },

      // ===== Crypto extras =====

      crypto_sha256: (data) => {
        const str = getString(data);
        const p = (typeof crypto !== 'undefined' && crypto.subtle)
          ? crypto.subtle.digest('SHA-256', new TextEncoder().encode(str))
              .then(buf => { const hex = [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join(''); stringTable.push(hex); return nanboxString(stringTable.length - 1); })
          : Promise.resolve(u64ToF64(TAG_UNDEFINED));
        return nanboxPointer(allocHandle(p));
      },
      crypto_md5: (data) => {
        // MD5 not available in Web Crypto API; return undefined
        // Users should use SHA-256 instead
        return u64ToF64(TAG_UNDEFINED);
      },

      // ===== Path extras =====

      path_is_absolute: (str) => {
        const s = getString(str);
        return (s.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(s)) ? 1 : 0;
      },

      // ===== Phase 5: Async/Promise/Fetch =====

      fetch_url: (urlStr) => {
        const url = getString(urlStr);
        const p = fetch(url);
        return nanboxPointer(allocHandle(p));
      },
      fetch_with_options: (urlStr, methodVal, bodyVal, headersVal) => {
        const url = getString(urlStr);
        const opts = {};
        if (!isUndefined(methodVal)) opts.method = getString(methodVal);
        if (!isUndefined(bodyVal)) opts.body = getString(bodyVal);
        if (isPointer(headersVal)) {
          const h = getHandle(headersVal);
          if (h && typeof h === 'object') opts.headers = h;
        }
        const p = fetch(url, opts);
        return nanboxPointer(allocHandle(p));
      },
      response_json: (handle) => {
        const resp = getHandle(handle);
        if (!resp || typeof resp.json !== 'function') return u64ToF64(TAG_UNDEFINED);
        const p = resp.json().then(v => fromJsValue(v));
        return nanboxPointer(allocHandle(p));
      },
      response_text: (handle) => {
        const resp = getHandle(handle);
        if (!resp || typeof resp.text !== 'function') return u64ToF64(TAG_UNDEFINED);
        const p = resp.text().then(v => fromJsValue(v));
        return nanboxPointer(allocHandle(p));
      },
      promise_new: () => {
        let resolve;
        const p = new Promise(r => { resolve = r; });
        p.__resolve = resolve;
        return nanboxPointer(allocHandle(p));
      },
      promise_resolve: (handle, value) => {
        const p = getHandle(handle);
        if (p && p.__resolve) p.__resolve(toJsValue(value));
      },
      promise_then: (handle, closureHandle) => {
        const p = getHandle(handle);
        const cb = getHandle(closureHandle);
        if (!p || !(p instanceof Promise)) return handle;
        const newP = p.then(val => {
          if (cb && wasmInstance) {
            const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
            if (fn) return u64ToF64(fn(...cb.captures, __jsValueToBits(val)));
          }
          return fromJsValue(val);
        });
        return nanboxPointer(allocHandle(newP));
      },
      await_promise: (val) => {
        // In synchronous WASM context, we can't truly await.
        // If it's not a promise handle, return as-is.
        // If it IS a promise, return the handle (caller gets a promise handle back).
        // True awaiting happens in JS async functions.
        if (!isPointer(val)) return val;
        const obj = getHandle(val);
        if (obj instanceof Promise) {
          // Can't await synchronously; return the promise handle
          return val;
        }
        return val;
      },

      // Memory-based bridge: ALL bridge function calls go through here.
      // Args are written to WASM memory at 0xFF00 as raw f64, preserving NaN payloads
      // that Firefox would canonicalize if passed as function parameters.
      // nameId = plain f64 string table index, argCount = number of f64 slots written.
      // Returns dummy f64 (0.0). Actual result is written to memory at 0xFF00.
      mem_call: (nameId, argCount, baseAddr) => {
        const name = stringTable[nameId | 0];
        const argc = argCount | 0;
        const base = baseAddr | 0;
        const u64View = new BigUint64Array(wasmMemory.buffer, base, Math.max(argc, 1));
        const args = [];
        for (let i = 0; i < argc; i++) args.push(__bitsToJsValue(u64View[i]));
        let result;
        const coreFn = __memDispatch[name];
        if (coreFn) {
          result = coreFn(...args);
        } else {
          const uiFn = __perryUiDispatch[name];
          if (uiFn) {
            result = uiFn(...args);
          } else if (argc > 0) {
            result = __classDispatch(args[0], name, args.slice(1));
          }
        }
        // Write result back to same base address
        const outU64 = new BigUint64Array(wasmMemory.buffer, base, 1);
        outU64[0] = __jsValueToBits(result);
        return 0;
      },
      mem_call_i32: (nameId, argCount, baseAddr) => {
        const name = stringTable[nameId | 0];
        const argc = argCount | 0;
        const base = baseAddr | 0;
        const u64View = new BigUint64Array(wasmMemory.buffer, base, Math.max(argc, 1));
        const args = [];
        for (let i = 0; i < argc; i++) args.push(__bitsToJsValue(u64View[i]));
        const fn = __memDispatch[name];
        if (!fn) return 0;
        return fn(...args) | 0;
      },

      // Async function implementations (merged from generated code)
      ...(typeof __asyncFuncImpls !== 'undefined' ? __asyncFuncImpls : {}),
    },
    // FFI namespace: external native functions (e.g., bloom_init_window, bloom_draw_rect)
    // Wrapped in Proxy so missing imports are auto-stubbed with no-ops returning
    // NaN-boxed undefined as BigInt, allowing the WASM module to instantiate.
    ffi: new Proxy(typeof __ffiImports !== 'undefined' ? __ffiImports : {}, {
      get(target, prop) {
        if (prop in target) return target[prop];
        return function ffiStub() { return TAG_UNDEFINED; };
      },
    }),
  };
}

// rt-namespace imports whose WASM-declared return type is NOT i64.
// `wrapForI64` MUST NOT BigInt-coerce Number returns for these — the
// WASM caller expects an i32 or f64, and a BigInt return throws
// "Cannot convert a BigInt value to a number" at the import boundary
// (#1049 instances 2/3). Built from the type tables in `emit.rs`:
// every entry with a `t_*_i32` signature plus the f64-return `mem_call`.
const __NON_I64_RETURN_IMPORTS = new Set([
  // i32 return
  'string_eq', 'is_truthy', 'js_strict_eq', 'is_null_or_undefined',
  'object_has_property', 'array_includes', 'array_is_array',
  'string_includes', 'string_startsWith', 'string_endsWith',
  'array_some', 'array_every', 'class_instanceof', 'map_has', 'set_has',
  'regexp_test', 'is_nan', 'is_finite', 'has_exception',
  'searchparams_has', 'response_ok', 'buffer_equals', 'buffer_is_buffer',
  'path_is_absolute',
  // f64 return — dummy 0, real result is in WASM memory
  'mem_call',
  // i32-return memory-bridge
  'mem_call_i32',
  // void return — wrapper passes through `undefined`, but listing here
  // makes the contract explicit and guards against future changes to the
  // wrapper that might try to coerce `0` for a void callee.
  'string_new', 'console_log', 'console_warn', 'console_error',
  'console_log_multi',
  'object_delete', 'object_delete_dynamic',
  'array_unshift', 'array_forEach',
  'try_start', 'try_end', 'throw_value',
  'map_clear', 'map_delete',
  'set_clear', 'set_add', 'set_delete',
  'searchparams_set', 'searchparams_append', 'searchparams_delete',
  'class_set_method', 'class_set_field', 'class_set_static', 'class_set_parent',
  'object_set_dynamic',
  'array_set', 'buffer_set', 'buffer_copy', 'buffer_write',
  'uint8array_set',
  'set_timeout', 'set_interval', 'clear_timeout', 'clear_interval',
  'promise_resolve',
]);

// Wrap a JS function so it interoperates with WASM i64 params/results.
//
// Default wrapping (used for the rt namespace): convert BigInt i64 args to f64
// via bit-reinterpret, so legacy rt.* functions that expect NaN-boxed f64 keep
// working. Number results are coerced back to BigInt for WASM i64 return.
// Necessary because BigInt(NaN) throws. Escape hatch: set `fn.__rawBigint = true`
// on the callee to skip the wrapper entirely and receive raw BigInt bits.
//
// `coerceNumberToBigInt` (default true) is set to false at the wrap site for
// imports whose declared WASM return is `i32`/`f64`/void — see
// `__NON_I64_RETURN_IMPORTS`. Without this guard, #1049 instance 1's
// `BigInt(result)` would mis-encode every i32-return import (is_truthy,
// js_strict_eq, mem_call_i32, …) and the f64-return `mem_call`, throwing
// "Cannot convert a BigInt value to a number" on the very first call
// (instances 2/3 reproduce as soon as `_start` invokes `class_set_method`
// via `mem_call`).
function wrapForI64(fn, coerceNumberToBigInt = true) {
  if (fn.__rawBigint === true) return fn;
  return function(...args) {
    const convertedArgs = args.map(a => {
      if (typeof a === 'bigint') { _u64[0] = a; return _f64[0]; }
      return a;
    });
    const result = fn.apply(this, convertedArgs);
    if (typeof result === 'bigint') return result;
    if (typeof result === 'number') {
      // #1049 instance 1: this wrapper exists because most callees here are
      // imported from a WASM site declared `i64`. A plain Number return —
      // even a small integer that JS would happily round-trip — throws
      // `TypeError: Cannot convert X to a BigInt` at the import boundary.
      // Coerce integers via `BigInt(n)` and non-integers (NaN-boxed f64
      // payloads) via bit-reinterpret. Skipped for non-i64-return imports
      // (#1049 instances 2/3) — those declare i32/f64 returns and would
      // reject a BigInt with the inverse error.
      if (coerceNumberToBigInt && Number.isInteger(result) && Math.abs(result) < 2147483648) {
        return BigInt(result);
      }
      if (!coerceNumberToBigInt) return result;
      _f64[0] = result;
      return _u64[0];
    }
    return result;
  };
}

// Wrapper for the ffi namespace: FFI imports are wasm-bindgen-generated functions
// and similar externals that expect PLAIN JS values, not NaN-boxed f64. Decode
// BigInt i64 args (NaN-box bits) to JS values via __bitsToJsValue before calling.
// Fixes Issue #133 items 3/5/6: Firefox/Safari canonicalize foreign NaNs when a
// Number crosses a JS function-call boundary, stripping STRING/POINTER/INT32 tags,
// so bit-reinterpreting BigInt → f64 here would lose the payload. Decoding bypasses
// the f64 round-trip entirely and also gives wasm-bindgen a plain Number for
// INT32-tagged values (avoiding `Cannot convert BigInt value to a number`).
function wrapFfiForI64(fn) {
  if (fn.__rawBigint === true) return fn;
  return function(...args) {
    const convertedArgs = args.map(a => (typeof a === 'bigint') ? __bitsToJsValue(a) : a);
    const result = fn.apply(this, convertedArgs);
    if (typeof result === 'bigint') return result;
    if (typeof result === 'number') {
      // #1049 instance 1: the WASM import side declares these as `i64`
      // returns (handles, NaN-box bits, etc.). A plain Number return —
      // even a small integer JS could round-trip — throws
      // `TypeError: Cannot convert X to a BigInt` at the import
      // boundary. NaN-box the Number via f64 bit-reinterpret for ALL
      // integers (not just non-integers): a raw `BigInt(1)` round-trips
      // as i64=1 which Perry then mis-reads as f64=5e-324 inside the WASM
      // body (#1049 instance 3 with `add_numbers` returning 7 surfacing
      // as `3.5e-323`). NaN-boxing via __jsValueToBits encodes the value
      // as a proper JSValue so downstream WASM code that reinterprets
      // i64 → f64 sees the original Number.
      return __jsValueToBits(result);
    }
    // Non-numeric result: encode via __jsValueToBits so callers that thread results
    // back into Perry get proper NaN-box bits.
    if (result === undefined) return TAG_UNDEFINED;
    return __jsValueToBits(result);
  };
}

function wrapNamespace(ns, wrapper) {
  return new Proxy(ns, {
    get(target, prop) {
      const v = target[prop];
      if (typeof v === 'function') return wrapper(v);
      return v;
    },
  });
}

// rt-namespace Proxy: pass each import's name into `wrapForI64` so it knows
// whether to BigInt-coerce Number returns. Names listed in
// `__NON_I64_RETURN_IMPORTS` have a non-i64 WASM-declared return (#1049
// instances 2/3) and must pass Numbers through unchanged.
function wrapRtNamespace(ns) {
  return new Proxy(ns, {
    get(target, prop) {
      const v = target[prop];
      if (typeof v === 'function') {
        const coerce = !__NON_I64_RETURN_IMPORTS.has(prop);
        return wrapForI64(v, coerce);
      }
      return v;
    },
  });
}

function wrapImportsForI64(imports) {
  const wrapped = {};
  for (const nsName in imports) {
    if (nsName === 'ffi') {
      wrapped[nsName] = wrapNamespace(imports[nsName], wrapFfiForI64);
    } else {
      wrapped[nsName] = wrapRtNamespace(imports[nsName]);
    }
  }
  return wrapped;
}

// Class method/static/parent tables
const classMethodTable = {};
const classStaticTable = {};
const classParentTable = {};

// Core bridge dispatch table — maps bridge function names to their implementations.
// Args are decoded from NaN-boxed BigInt bits to plain JS values by __bitsToJsValue before dispatch.
// Return values are plain JS values, converted back to BigInt bits by __jsValueToBits in mem_call.
const __memDispatch = {
  // Console — args are already decoded by __bitsToJsValue
  console_log: (val) => { console.log(val); },
  console_warn: (val) => { console.warn(val); },
  console_error: (val) => { console.error(val); },
  console_log_multi: (args) => { if (Array.isArray(args)) console.log(...args); },

  // String core — args are already decoded JS values via __bitsToJsValue
  string_concat: (a, b) => String(a) + String(b),
  js_add: (a, b) => a + b,
  string_eq: (a, b) => a === b ? 1 : 0,
  string_len: (val) => {
    if (typeof val === 'string') return val.length;
    if (Array.isArray(val)) return val.length;
    return 0;
  },
  jsvalue_to_string: (val) => String(val),
  is_truthy: (val) => {
    if (val === false || val === null || val === undefined || val === 0 || val === '') return 0;
    if (Number.isNaN(val)) return 0;
    return 1;
  },
  js_strict_eq: (a, b) => a === b ? 1 : 0,

  // Math
  math_floor: (x) => Math.floor(x),
  math_ceil: (x) => Math.ceil(x),
  math_round: (x) => Math.round(x),
  math_abs: (x) => Math.abs(x),
  math_sqrt: (x) => Math.sqrt(x),
  math_pow: (base, exp) => Math.pow(base, exp),
  math_random: () => Math.random(),
  math_log: (x) => Math.log(x),
  math_log2: (x) => Math.log2(x),
  math_log10: (x) => Math.log10(x),
  math_min: (a, b) => Math.min(a, b),
  math_max: (a, b) => Math.max(a, b),
  // Trig / exp / sign / trunc / cbrt / hypot / etc. (Issue #133 item 4)
  math_sin: (x) => Math.sin(x),
  math_cos: (x) => Math.cos(x),
  math_tan: (x) => Math.tan(x),
  math_asin: (x) => Math.asin(x),
  math_acos: (x) => Math.acos(x),
  math_atan: (x) => Math.atan(x),
  math_atan2: (y, x) => Math.atan2(y, x),
  math_sinh: (x) => Math.sinh(x),
  math_cosh: (x) => Math.cosh(x),
  math_tanh: (x) => Math.tanh(x),
  math_asinh: (x) => Math.asinh(x),
  math_acosh: (x) => Math.acosh(x),
  math_atanh: (x) => Math.atanh(x),
  math_exp: (x) => Math.exp(x),
  math_expm1: (x) => Math.expm1(x),
  math_log1p: (x) => Math.log1p(x),
  math_sign: (x) => Math.sign(x),
  math_trunc: (x) => Math.trunc(x),
  math_cbrt: (x) => Math.cbrt(x),
  math_hypot: (a, b) => Math.hypot(a, b),
  math_fround: (x) => Math.fround(x),
  math_clz32: (x) => Math.clz32(x),
  math_imul: (a, b) => Math.imul(a, b),
  date_now: () => Date.now(),
  js_typeof: (val) => typeof val,
  parse_int: (val) => parseInt(String(val), 10),
  parse_float: (val) => parseFloat(String(val)),
  js_mod: (a, b) => a % b,
  is_null_or_undefined: (val) => (val === null || val === undefined) ? 1 : 0,

  // Objects — args are plain JS values (obj is the object itself, key is a string, etc.)
  object_new: () => ({}),
  object_set: (obj, key, value) => {
    if (obj && typeof obj === 'object') obj[String(key)] = value; return obj;
  },
  object_get: (obj, key) => {
    if (!obj || typeof obj !== 'object') return undefined;
    return obj[String(key)];
  },
  object_get_dynamic: (obj, key) => {
    if (!obj || typeof obj !== 'object') return undefined;
    return obj[key];
  },
  object_set_dynamic: (obj, key, value) => {
    if (obj && typeof obj === 'object') obj[key] = value;
  },
  object_delete: (obj, key) => { if (obj && typeof obj === 'object') delete obj[String(key)]; },
  object_delete_dynamic: (obj, key) => { if (obj && typeof obj === 'object') delete obj[key]; },
  object_keys: (obj) => obj && typeof obj === 'object' ? Object.keys(obj).filter(k => k !== "__class__") : [],
  object_values: (obj) => obj && typeof obj === 'object' ? Object.keys(obj).filter(k => k !== "__class__").map(k => obj[k]) : [],
  object_entries: (obj) => obj && typeof obj === 'object' ? Object.entries(obj).filter(([k]) => k !== "__class__") : [],
  object_has_property: (obj, key) => {
    if (!obj || typeof obj !== 'object') return 0;
    return (key in obj) ? 1 : 0;
  },
  object_assign: (target, source) => {
    if (target == null) throw new TypeError('Cannot convert undefined or null to object');
    const t = Object(target);
    if (source != null) Object.assign(t, source);
    return t;
  },

  // Arrays — args are plain JS values (arr is the array itself, etc.)
  array_new: () => [],
  array_push: (arr, value) => { if (Array.isArray(arr)) arr.push(value); return arr; },
  array_pop: (arr) => { if (!Array.isArray(arr) || arr.length === 0) return undefined; return arr.pop(); },
  array_get: (arr, index) => {
    if (!Array.isArray(arr)) return undefined;
    return arr[typeof index === 'number' ? index : index];
  },
  array_set: (arr, index, value) => { if (Array.isArray(arr)) arr[typeof index === 'number' ? index : index] = value; },
  array_length: (arr) => Array.isArray(arr) ? arr.length : 0,
  array_slice: (arr, start, end) => {
    if (!Array.isArray(arr)) return [];
    const s = typeof start === 'number' ? start : 0;
    const e = end === undefined ? undefined : (typeof end === 'number' ? end : end);
    return arr.slice(s, e);
  },
  array_splice: (arr, start, deleteCount) => {
    if (!Array.isArray(arr)) return [];
    const s = typeof start === 'number' ? start : 0;
    const dc = deleteCount === undefined ? arr.length - s : deleteCount;
    return arr.splice(s, dc);
  },
  array_shift: (arr) => { if (!Array.isArray(arr) || arr.length === 0) return undefined; return arr.shift(); },
  array_unshift: (arr, value) => { if (Array.isArray(arr)) arr.unshift(value); },
  array_join: (arr, separator) => {
    if (!Array.isArray(arr)) return '';
    const sep = String(separator);
    return arr.map(v => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)).join(sep);
  },
  array_index_of: (arr, value) => { if (!Array.isArray(arr)) return -1; return arr.indexOf(value); },
  array_includes: (arr, value) => { if (!Array.isArray(arr)) return 0; return arr.includes(value) ? 1 : 0; },
  array_concat: (a1, a2) => (Array.isArray(a1) ? a1 : []).concat(Array.isArray(a2) ? a2 : []),
  array_reverse: (arr) => { if (Array.isArray(arr)) arr.reverse(); return arr; },
  array_flat: (arr) => Array.isArray(arr) ? arr.flat() : [],
  array_is_array: (val) => Array.isArray(val) ? 1 : 0,
  array_from: (val) => Array.from(val),
  array_push_spread: (target, source) => { if (Array.isArray(target) && Array.isArray(source)) target.push(...source); return target; },

  // String methods — args are plain JS strings
  string_charAt: (str, idx) => String(str).charAt(idx),
  string_substring: (str, start, end) => String(str).substring(start, end),
  string_indexOf: (str, search) => String(str).indexOf(String(search)),
  string_slice: (str, start, end) => String(str).slice(start, end),
  string_toLowerCase: (str) => String(str).toLowerCase(),
  string_toUpperCase: (str) => String(str).toUpperCase(),
  string_trim: (str) => String(str).trim(),
  string_includes: (str, search) => String(str).includes(String(search)) ? 1 : 0,
  string_startsWith: (str, search) => String(str).startsWith(String(search)) ? 1 : 0,
  string_endsWith: (str, search) => String(str).endsWith(String(search)) ? 1 : 0,
  string_replace: (str, pattern, replacement) => {
    const s = String(str);
    const p = (pattern instanceof RegExp) ? pattern : String(pattern);
    return s.replace(p, String(replacement));
  },
  string_split: (str, delim) => String(str).split(String(delim)),
  string_fromCharCode: (code) => String.fromCharCode(code),
  string_padStart: (str, len, fill) => String(str).padStart(len, String(fill)),
  string_padEnd: (str, len, fill) => String(str).padEnd(len, String(fill)),
  string_repeat: (str, count) => String(str).repeat(count),
  string_match: (str, regex) => {
    const s = String(str);
    const re = (regex instanceof RegExp) ? regex : new RegExp(String(regex));
    const result = s.match(re);
    if (!result) return null;
    return Array.from(result);
  },

  // Closures — WASM functions now use i64 params/returns (BigInt in JS).
  // Captures are stored as BigInt (i64 NaN-boxed bits).
  // __jsValueToBits converts JS values to BigInt for WASM calls.
  // __bitsToJsValue converts BigInt return values back to JS.
  closure_new: (funcIdx, captureCount) => ({ funcIdx: funcIdx, captures: new Array(captureCount | 0) }),
  closure_set_capture: (closure, idx, value) => {
    if (closure) closure.captures[idx | 0] = __jsValueToBits(value);
    return closure;
  },
  closure_call_0: (closure) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    // Pad for omitted TS optional params (Issue #1035).
    const padded = __padBigintArgs(fn, closure.captures.length, []);
    return __bitsToJsValue(fn(...closure.captures, ...padded));
  },
  closure_call_1: (closure, a0) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    const padded = __padBigintArgs(fn, closure.captures.length + 1, [__jsValueToBits(a0)]);
    return __bitsToJsValue(fn(...closure.captures, ...padded));
  },
  closure_call_2: (closure, a0, a1) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    const padded = __padBigintArgs(fn, closure.captures.length + 2, [__jsValueToBits(a0), __jsValueToBits(a1)]);
    return __bitsToJsValue(fn(...closure.captures, ...padded));
  },
  closure_call_3: (closure, a0, a1, a2) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    const padded = __padBigintArgs(fn, closure.captures.length + 3, [__jsValueToBits(a0), __jsValueToBits(a1), __jsValueToBits(a2)]);
    return __bitsToJsValue(fn(...closure.captures, ...padded));
  },
  closure_call_spread: (closure, args) => {
    const argArr = Array.isArray(args) ? args : [];
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    const padded = __padBigintArgs(fn, closure.captures.length, argArr.map(v => __jsValueToBits(v)));
    return __bitsToJsValue(fn(...closure.captures, ...padded));
  },

  // Array higher-order methods — WASM callbacks use i64 (BigInt) params/returns.
  array_map: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return [];
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return [];
    return arr.map((v, i) => __bitsToJsValue(fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i))));
  },
  array_filter: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return [];
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return [];
    return arr.filter((v, i) => {
      const r = fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i));
      const rv = __bitsToJsValue(r);
      if (rv === true) return true;
      if (rv === false || rv === null || rv === undefined) return false;
      return !!rv;
    });
  },
  array_forEach: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return;
    arr.forEach((v, i) => fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)));
  },
  array_reduce: (arr, cb, initial) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return undefined;
    let acc = initial === undefined ? __jsValueToBits(arr[0]) : __jsValueToBits(initial);
    const startIdx = initial === undefined ? 1 : 0;
    for (let i = startIdx; i < arr.length; i++) acc = fn(...cb.captures, acc, __jsValueToBits(arr[i]), __jsValueToBits(i));
    return __bitsToJsValue(acc);
  },
  array_find: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return undefined;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return undefined;
    for (let i = 0; i < arr.length; i++) {
      const vBits = __jsValueToBits(arr[i]);
      if (__bitsToJsValue(fn(...cb.captures, vBits, __jsValueToBits(i)))) return arr[i];
    }
    return undefined;
  },
  array_find_index: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return -1;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return -1;
    for (let i = 0; i < arr.length; i++) { if (__bitsToJsValue(fn(...cb.captures, __jsValueToBits(arr[i]), __jsValueToBits(i)))) return i; }
    return -1;
  },
  array_sort: (arr, cb) => {
    if (!Array.isArray(arr)) return arr;
    if (cb === undefined || cb === null) { arr.sort(); return arr; }
    if (!cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) { arr.sort(); return arr; }
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) { arr.sort(); return arr; }
    arr.sort((a, b) => { const r = fn(...cb.captures, __jsValueToBits(a), __jsValueToBits(b)); return __bitsToJsValue(r); });
    return arr;
  },
  array_some: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return 0;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return 0;
    return arr.some((v, i) => __bitsToJsValue(fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)))) ? 1 : 0;
  },
  array_every: (arr, cb) => {
    if (!Array.isArray(arr) || !cb || typeof cb.funcIdx === 'undefined' || !wasmInstance) return 0;
    const fn = wasmInstance.exports.__indirect_function_table?.get(cb.funcIdx | 0);
    if (!fn) return 0;
    return arr.every((v, i) => __bitsToJsValue(fn(...cb.captures, __jsValueToBits(v), __jsValueToBits(i)))) ? 1 : 0;
  },

  // Classes — args are plain JS values. obj is the object, className/methodName are strings.
  // class_new returns a plain object with __class__ property.
  // class_set_method/class_call_method interact with WASM indirect call table.
  class_new: (className, fieldCount) => { const obj = {}; obj.__class__ = String(className); return obj; },
  class_set_method: (classId, methodName, funcIdx) => {
    const cls = String(classId);
    if (!classMethodTable[cls]) classMethodTable[cls] = {};
    classMethodTable[cls][String(methodName)] = funcIdx | 0;
  },
  class_call_method: (obj, methodNameId, argsArr) => {
    // methodNameId is a plain number (string table index), not a decoded JS value
    const mname = stringTable[methodNameId | 0];
    if (obj && obj.__class__) {
      let cls = obj.__class__;
      while (cls) {
        const methods = classMethodTable[cls];
        if (methods && mname in methods) {
          const fn = wasmInstance?.exports.__indirect_function_table?.get(methods[mname]);
          if (fn) {
            const args = Array.isArray(argsArr) ? argsArr : [];
            // WASM functions use i64 (BigInt) params/returns.
            // Pad for omitted TS optional params (Issue #1035).
            const padded = __padBigintArgs(fn, 1, args.map(v => __jsValueToBits(v)));
            return __bitsToJsValue(fn(__jsValueToBits(obj), ...padded));
          }
        }
        cls = classParentTable[cls] || null;
      }
    }
    // Fallback to UI method dispatch
    const uiMethodMap = {
      addChild: "perry_ui_widget_add_child", removeAllChildren: "perry_ui_widget_remove_all_children",
      setBackground: "perry_ui_set_background", setForeground: "perry_ui_set_foreground",
      setFontSize: "perry_ui_set_font_size", setFontWeight: "perry_ui_set_font_weight",
      setFontFamily: "perry_ui_set_font_family", setPadding: "perry_ui_set_padding",
      setFrame: "perry_ui_set_frame", setCornerRadius: "perry_ui_set_corner_radius",
      setBorder: "perry_ui_set_border", setOpacity: "perry_ui_set_opacity",
      setEnabled: "perry_ui_set_enabled", setTooltip: "perry_ui_set_tooltip",
      setControlSize: "perry_ui_set_control_size",
      animateOpacity: "perry_ui_animate_opacity", animatePosition: "perry_ui_animate_position",
      setOnClick: "perry_ui_set_on_click", setOnHover: "perry_ui_set_on_hover",
      setOnDoubleClick: "perry_ui_set_on_double_click",
      get: "perry_ui_state_get", set: "perry_ui_state_set", create: "perry_ui_state_create",
      bindText: "perry_ui_state_bind_text", bindTextNumeric: "perry_ui_state_bind_text_numeric",
      bindSlider: "perry_ui_state_bind_slider", bindToggle: "perry_ui_state_bind_toggle",
      bindVisibility: "perry_ui_state_bind_visibility", bindForEach: "perry_ui_state_bind_foreach",
      onChange: "perry_ui_state_on_change",
      setString: "perry_ui_text_set_string", setSelectable: "perry_ui_text_set_selectable",
      setBordered: "perry_ui_button_set_bordered", setTitle: "perry_ui_button_set_title",
      setTextColor: "perry_ui_button_set_text_color", setImage: "perry_ui_button_set_image",
      fillRect: "perry_ui_canvas_fill_rect", strokeRect: "perry_ui_canvas_stroke_rect",
      clearRect: "perry_ui_canvas_clear_rect", setFillColor: "perry_ui_canvas_set_fill_color",
      setStrokeColor: "perry_ui_canvas_set_stroke_color", beginPath: "perry_ui_canvas_begin_path",
      moveTo: "perry_ui_canvas_move_to", lineTo: "perry_ui_canvas_line_to",
      arc: "perry_ui_canvas_arc", closePath: "perry_ui_canvas_close_path",
      fill: "perry_ui_canvas_fill", stroke: "perry_ui_canvas_stroke",
      setLineWidth: "perry_ui_canvas_set_line_width", fillText: "perry_ui_canvas_fill_text",
      setFont: "perry_ui_canvas_set_font", drawImage: "perry_ui_canvas_draw_image",
      setChild: "perry_ui_scrollview_set_child", focus: "perry_ui_textfield_focus",
      setWidth: "perry_ui_widget_set_width", setHeight: "perry_ui_widget_set_height",
      matchParentWidth: "perry_ui_widget_match_parent_width",
      matchParentHeight: "perry_ui_widget_match_parent_height",
      setHidden: "perry_ui_set_widget_hidden", setEdgeInsets: "perry_ui_widget_set_edge_insets",
      setShadow: "perry_ui_widget_set_shadow",
      run: "perry_ui_app_run", setBody: "perry_ui_app_set_body",
    };
    const uiFnName = uiMethodMap[mname];
    if (uiFnName) {
      const fn = __perryUiDispatch[uiFnName]; if (!fn) return undefined;
      const args = Array.isArray(argsArr) ? argsArr : [];
      return fn(obj, ...args);
    }
    return undefined;
  },
  class_get_field: (obj, name) => {
    if (!obj || typeof obj !== 'object') return undefined;
    const fname = String(name);
    // Check for compiled getter method — WASM uses i64 (BigInt)
    if (obj.__class__) {
      let cls = obj.__class__;
      while (cls) {
        const methods = classMethodTable[cls];
        if (methods && ('__get_' + fname) in methods) {
          const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__get_' + fname]);
          if (fn) return __bitsToJsValue(fn(__jsValueToBits(obj)));
        }
        cls = classParentTable[cls] || null;
      }
    }
    return obj[fname];
  },
  class_set_field: (obj, name, value) => {
    if (!obj || typeof obj !== 'object') return;
    const fname = String(name);
    // Check for compiled setter method — WASM uses i64 (BigInt)
    if (obj.__class__) {
      let cls = obj.__class__;
      while (cls) {
        const methods = classMethodTable[cls];
        if (methods && ('__set_' + fname) in methods) {
          const fn = wasmInstance?.exports.__indirect_function_table?.get(methods['__set_' + fname]);
          if (fn) { fn(__jsValueToBits(obj), __jsValueToBits(value)); return; }
        }
        cls = classParentTable[cls] || null;
      }
    }
    obj[fname] = value;
  },
  class_set_static: (classId, name, value) => {
    const cls = String(classId);
    if (!classStaticTable[cls]) classStaticTable[cls] = {};
    classStaticTable[cls][String(name)] = value;
  },
  class_get_static: (classId, name) => {
    const cls = String(classId); const statics = classStaticTable[cls];
    if (!statics) return undefined;
    return statics[String(name)];
  },
  class_instanceof: (obj, classId) => {
    if (!obj || typeof obj !== 'object') return 0;
    let cls = obj.__class__; const target = String(classId);
    while (cls) { if (cls === target) return 1; cls = classParentTable[cls] || null; }
    return 0;
  },
  class_set_parent: (childId, parentId) => { classParentTable[String(childId)] = String(parentId); },

  // JSON — args are plain JS values
  json_parse: (str) => { try { return JSON.parse(String(str)); } catch (e) { if (tryDepth > 0) currentException = e; return undefined; } },
  json_stringify: (val) => JSON.stringify(val),

  // Map — args are plain JS values (m is the Map object itself)
  map_new: () => new Map(),
  map_set: (m, key, value) => { if (m instanceof Map) m.set(key, value); },
  map_get: (m, key) => { if (!(m instanceof Map)) return undefined; return m.get(key); },
  map_has: (m, key) => (m instanceof Map && m.has(key)) ? 1 : 0,
  map_delete: (m, key) => { if (m instanceof Map) m.delete(key); },
  map_size: (m) => (m instanceof Map) ? m.size : 0,
  map_clear: (m) => { if (m instanceof Map) m.clear(); },
  map_entries: (m) => (m instanceof Map) ? [...m.entries()] : [],
  map_keys: (m) => (m instanceof Map) ? [...m.keys()] : [],
  map_values: (m) => (m instanceof Map) ? [...m.values()] : [],

  // Set — args are plain JS values (s is the Set object itself)
  set_new: () => new Set(),
  set_new_from_array: (arr) => new Set(Array.isArray(arr) ? arr : []),
  set_add: (s, value) => { if (s instanceof Set) s.add(value); },
  set_has: (s, value) => (s instanceof Set && s.has(value)) ? 1 : 0,
  set_delete: (s, value) => { if (s instanceof Set) s.delete(value); },
  set_size: (s) => (s instanceof Set) ? s.size : 0,
  set_clear: (s) => { if (s instanceof Set) s.clear(); },
  set_values: (s) => (s instanceof Set) ? [...s.values()] : [],

  // Date — arg is a plain JS value (the Date object itself once created)
  date_new: () => new Date(),
  date_new_val: (arg) => (arg === undefined) ? new Date() : new Date(arg),
  date_now: () => Date.now(),
  date_parse: (s) => Date.parse(String(s)),
  date_utc: (...args) => Date.UTC(...args.map(Number)),
  date_get_time: (d) => (d instanceof Date) ? d.getTime() : 0,
  date_value_of: (d) => (d instanceof Date) ? d.valueOf() : 0,
  date_to_iso_string: (d) => { if (!(d instanceof Date)) return undefined; return d.toISOString(); },
  date_to_date_string: (d) => (d instanceof Date) ? d.toDateString() : '',
  date_to_time_string: (d) => (d instanceof Date) ? d.toTimeString() : '',
  date_to_locale_string: (d) => (d instanceof Date) ? d.toLocaleString() : '',
  date_to_locale_date_string: (d) => (d instanceof Date) ? d.toLocaleDateString() : '',
  date_to_locale_time_string: (d) => (d instanceof Date) ? d.toLocaleTimeString() : '',
  date_to_json: (d) => (d instanceof Date) ? d.toJSON() : null,
  date_get_full_year: (d) => (d instanceof Date) ? d.getFullYear() : 0,
  date_get_month: (d) => (d instanceof Date) ? d.getMonth() : 0,
  date_get_date: (d) => (d instanceof Date) ? d.getDate() : 0,
  date_get_day: (d) => (d instanceof Date) ? d.getDay() : 0,
  date_get_hours: (d) => (d instanceof Date) ? d.getHours() : 0,
  date_get_minutes: (d) => (d instanceof Date) ? d.getMinutes() : 0,
  date_get_seconds: (d) => (d instanceof Date) ? d.getSeconds() : 0,
  date_get_milliseconds: (d) => (d instanceof Date) ? d.getMilliseconds() : 0,
  date_get_utc_full_year: (d) => (d instanceof Date) ? d.getUTCFullYear() : 0,
  date_get_utc_month: (d) => (d instanceof Date) ? d.getUTCMonth() : 0,
  date_get_utc_date: (d) => (d instanceof Date) ? d.getUTCDate() : 0,
  date_get_utc_day: (d) => (d instanceof Date) ? d.getUTCDay() : 0,
  date_get_utc_hours: (d) => (d instanceof Date) ? d.getUTCHours() : 0,
  date_get_utc_minutes: (d) => (d instanceof Date) ? d.getUTCMinutes() : 0,
  date_get_utc_seconds: (d) => (d instanceof Date) ? d.getUTCSeconds() : 0,
  date_get_utc_milliseconds: (d) => (d instanceof Date) ? d.getUTCMilliseconds() : 0,
  date_get_timezone_offset: (d) => (d instanceof Date) ? d.getTimezoneOffset() : 0,
  date_set_utc_full_year: (d, v) => (d instanceof Date) ? d.setUTCFullYear(Number(v)) : 0,
  date_set_utc_month: (d, v) => (d instanceof Date) ? d.setUTCMonth(Number(v)) : 0,
  date_set_utc_date: (d, v) => (d instanceof Date) ? d.setUTCDate(Number(v)) : 0,
  date_set_utc_hours: (d, v) => (d instanceof Date) ? d.setUTCHours(Number(v)) : 0,
  date_set_utc_minutes: (d, v) => (d instanceof Date) ? d.setUTCMinutes(Number(v)) : 0,
  date_set_utc_seconds: (d, v) => (d instanceof Date) ? d.setUTCSeconds(Number(v)) : 0,
  date_set_utc_milliseconds: (d, v) => (d instanceof Date) ? d.setUTCMilliseconds(Number(v)) : 0,

  // Error — args are plain JS values
  error_new: (msg) => new Error(msg === undefined ? undefined : String(msg)),
  error_message: (e) => (e instanceof Error) ? e.message : '',

  // RegExp — args are plain JS values
  regexp_new: (pattern, flags) => { try { return new RegExp(String(pattern), String(flags)); } catch (e) { if (tryDepth > 0) currentException = e; return undefined; } },
  regexp_test: (re, str) => { if (!(re instanceof RegExp)) return 0; return re.test(String(str)) ? 1 : 0; },

  // Globals — args are plain JS values
  number_coerce: (val) => Number(val),
  is_nan: (val) => Number.isNaN(val) ? 1 : 0,
  is_finite: (val) => Number.isFinite(val) ? 1 : 0,

  // Try/Catch — args are plain JS values
  try_start: () => { tryDepth++; },
  try_end: () => { if (tryDepth > 0) tryDepth--; },
  throw_value: (val) => { currentException = val; },
  has_exception: () => currentException !== null ? 1 : 0,
  get_exception: () => { const e = currentException; currentException = null; return e !== null ? e : undefined; },

  // URL — args are plain JS values (u is the URL object itself)
  url_parse: (urlStr) => { try { return new URL(String(urlStr)); } catch (e) { if (tryDepth > 0) currentException = e; return undefined; } },
  url_get_href: (u) => (u instanceof URL) ? u.href : undefined,
  url_get_pathname: (u) => (u instanceof URL) ? u.pathname : undefined,
  url_get_hostname: (u) => (u instanceof URL) ? u.hostname : undefined,
  url_get_port: (u) => (u instanceof URL) ? u.port : undefined,
  url_get_search: (u) => (u instanceof URL) ? u.search : undefined,
  url_get_hash: (u) => (u instanceof URL) ? u.hash : undefined,
  url_get_origin: (u) => (u instanceof URL) ? u.origin : undefined,
  url_get_protocol: (u) => (u instanceof URL) ? u.protocol : undefined,
  url_get_search_params: (u) => (u instanceof URL) ? u.searchParams : undefined,
  searchparams_get: (sp, key) => { if (!sp || typeof sp.get !== 'function') return null; const v = sp.get(String(key)); return v === null ? null : v; },
  searchparams_has: (sp, key) => (sp && typeof sp.has === 'function' && sp.has(String(key))) ? 1 : 0,
  searchparams_set: (sp, key, val) => { if (sp && typeof sp.set === 'function') sp.set(String(key), String(val)); },
  searchparams_append: (sp, key, val) => { if (sp && typeof sp.append === 'function') sp.append(String(key), String(val)); },
  searchparams_delete: (sp, key) => { if (sp && typeof sp.delete === 'function') sp.delete(String(key)); },
  searchparams_to_string: (sp) => sp ? sp.toString() : '',

  // Crypto — args are plain JS values
  crypto_random_uuid: () => crypto.randomUUID(),
  crypto_random_uuidv7: () => crypto.randomUUIDv7(),
  crypto_random_bytes: (n) => crypto.getRandomValues(new Uint8Array(n)),
  crypto_sha256: (data) => {
    const str = String(data);
    const p = (typeof crypto !== 'undefined' && crypto.subtle)
      ? crypto.subtle.digest('SHA-256', new TextEncoder().encode(str))
          .then(buf => [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join(''))
      : Promise.resolve(undefined);
    return p;
  },
  crypto_md5: () => undefined,

  // Path — args are plain JS strings
  path_join: (a, b) => (String(a) + '/' + String(b)).replace(/\/+/g, '/'),
  path_dirname: (str) => { const s = String(str); const idx = s.lastIndexOf('/'); return idx >= 0 ? (s.substring(0, idx) || '/') : '.'; },
  path_basename: (str) => { const s = String(str); const idx = s.lastIndexOf('/'); return idx >= 0 ? s.substring(idx + 1) : s; },
  path_extname: (str) => { const s = String(str); const base = s.substring(s.lastIndexOf('/') + 1); const idx = base.lastIndexOf('.'); return idx > 0 ? base.substring(idx) : ''; },
  path_resolve: (str) => String(str),
  path_is_absolute: (str) => { const s = String(str); return (s.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(s)) ? 1 : 0; },

  // Process/OS — return plain JS values
  os_platform: () => 'wasm',
  process_argv: () => [],
  process_cwd: () => '/',

  // Buffer/Uint8Array — args are plain JS values (buf is the Uint8Array itself)
  buffer_alloc: (size) => new Uint8Array(size),
  buffer_from_string: (str, encoding) => new TextEncoder().encode(String(str)),
  buffer_to_string: (buf, encoding) => { if (!buf || !(buf instanceof Uint8Array)) return ''; return new TextDecoder().decode(buf); },
  buffer_get: (buf, idx) => (buf instanceof Uint8Array) ? buf[idx] : 0,
  buffer_set: (buf, idx, val) => { if (buf instanceof Uint8Array) buf[idx] = val; },
  buffer_length: (buf) => (buf instanceof Uint8Array) ? buf.length : 0,
  buffer_slice: (buf, start, end) => { if (!(buf instanceof Uint8Array)) return new Uint8Array(0); return buf.slice(start, end === undefined ? undefined : end); },
  buffer_concat: (arr) => {
    if (!Array.isArray(arr)) return new Uint8Array(0);
    const bufs = arr.map(v => (v instanceof Uint8Array) ? v : new Uint8Array(0));
    const total = bufs.reduce((s, b) => s + b.length, 0);
    const result = new Uint8Array(total); let offset = 0;
    bufs.forEach(b => { result.set(b, offset); offset += b.length; });
    return result;
  },
  uint8array_new: (size) => new Uint8Array(size),
  uint8array_from: (val) => Uint8Array.from(Array.isArray(val) ? val : []),
  uint8array_length: (buf) => (buf instanceof Uint8Array) ? buf.length : 0,
  uint8array_get: (buf, idx) => (buf instanceof Uint8Array) ? buf[idx] : 0,
  uint8array_set: (buf, idx, val) => { if (buf instanceof Uint8Array) buf[idx] = val; },

  // Timers — closure arg is the closure object itself (decoded from POINTER_TAG)
  // Captures are already BigInt from closure_set_capture.
  set_timeout: (closure, delay) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return 0;
    return setTimeout(() => { const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0); if (fn) fn(...closure.captures); }, delay);
  },
  set_interval: (closure, delay) => {
    if (!closure || typeof closure.funcIdx === 'undefined' || !wasmInstance) return 0;
    return setInterval(() => { const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0); if (fn) fn(...closure.captures); }, delay);
  },
  clear_timeout: (id) => { clearTimeout(id); },
  clear_interval: (id) => { clearInterval(id); },

  // Response — args are plain JS values (r is the Response object itself)
  response_status: (r) => (r && typeof r.status === 'number') ? r.status : 0,
  response_ok: (r) => (r && r.ok) ? 1 : 0,
  response_headers_get: (r, name) => {
    if (!r || !r.headers) return null;
    const v = r.headers.get(String(name));
    return v === null ? null : v;
  },
  response_url: (r) => (r && typeof r.url === 'string') ? r.url : undefined,

  // Buffer extras — args are plain JS values (src/tgt are Uint8Array objects)
  buffer_copy: (src, tgt, targetStart, sourceStart, sourceEnd) => {
    if (!(src instanceof Uint8Array) || !(tgt instanceof Uint8Array)) return 0;
    const ts = targetStart === undefined ? 0 : targetStart;
    const ss = sourceStart === undefined ? 0 : sourceStart;
    const se = sourceEnd === undefined ? src.length : sourceEnd;
    let copied = 0;
    for (let i = ss; i < se && (ts + copied) < tgt.length; i++) { tgt[ts + copied] = src[i]; copied++; }
    return copied;
  },
  buffer_write: (buf, str, offset, encoding) => {
    if (!(buf instanceof Uint8Array)) return 0;
    const encoded = new TextEncoder().encode(String(str));
    const off = offset === undefined ? 0 : offset; let written = 0;
    for (let i = 0; i < encoded.length && (off + i) < buf.length; i++) { buf[off + i] = encoded[i]; written++; }
    return written;
  },
  buffer_equals: (a, b) => { if (!(a instanceof Uint8Array) || !(b instanceof Uint8Array) || a.length !== b.length) return 0; for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return 0; return 1; },
  buffer_is_buffer: (val) => (val instanceof Uint8Array || (val && val.constructor && val.constructor.name === 'Buffer')) ? 1 : 0,
  buffer_byte_length: (val) => { if (typeof val === 'string') return new TextEncoder().encode(val).length; if (val instanceof Uint8Array) return val.length; return 0; },

  // Fetch/Promise — args are plain JS values
  fetch_url: (urlStr) => fetch(String(urlStr)),
  fetch_with_options: (urlStr, methodVal, bodyVal, headersVal) => {
    const url = String(urlStr); const opts = {};
    if (methodVal !== undefined) opts.method = String(methodVal);
    if (bodyVal !== undefined) opts.body = String(bodyVal);
    if (headersVal && typeof headersVal === 'object') opts.headers = headersVal;
    return fetch(url, opts);
  },
  response_json: (resp) => { if (!resp || typeof resp.json !== 'function') return undefined; return resp.json(); },
  response_text: (resp) => { if (!resp || typeof resp.text !== 'function') return undefined; return resp.text(); },
  promise_new: () => { let resolve; const p = new Promise(r => { resolve = r; }); p.__resolve = resolve; return p; },
  promise_resolve: (p, value) => { if (p && p.__resolve) p.__resolve(value); },
  promise_then: (p, closure) => {
    if (!p || !(p instanceof Promise)) return p;
    const newP = p.then(val => {
      if (closure && typeof closure.funcIdx !== 'undefined' && wasmInstance) {
        const fn = wasmInstance.exports.__indirect_function_table?.get(closure.funcIdx | 0);
        if (fn) return __bitsToJsValue(fn(...closure.captures, __jsValueToBits(val)));
      }
      return val;
    });
    return newP;
  },
  await_promise: (val) => val,

  // === perry/thread — Web Worker parallelism ===
  thread_parallel_map: (array, closure) => {
    if (!Array.isArray(array)) return [];
    return __threadPool.map(array, closure);
  },
  thread_parallel_filter: (array, closure) => {
    if (!Array.isArray(array)) return [];
    return __threadPool.filter(array, closure);
  },
  thread_spawn: (closure) => {
    return __threadPool.spawn(closure);
  },
};

// --- perry/thread Worker Pool ---
// Manages a pool of Web Workers that each instantiate the WASM module.
// Workers call compiled WASM functions via the indirect function table.
const __threadPool = (() => {
  const MIN_PARALLEL_SIZE = 1024;
  let workers = null;
  let wasmBinary = null;

  // Minimal runtime bridge for workers — they need NaN-boxing, string table, handle store
  const WORKER_BOOTSTRAP = `
"use strict";
const TAG_UNDEFINED = 0x7FFC0000_00000001n;
const TAG_NULL      = 0x7FFC0000_00000002n;
const TAG_FALSE     = 0x7FFC0000_00000003n;
const TAG_TRUE      = 0x7FFC0000_00000004n;
const STRING_TAG    = 0x7FFFn;
const POINTER_TAG   = 0x7FFDn;
const INT32_TAG     = 0x7FFEn;
const _convBuf = new ArrayBuffer(8);
const _f64 = new Float64Array(_convBuf);
const _u64 = new BigUint64Array(_convBuf);
function f64ToU64(f) { _f64[0] = f; return _u64[0]; }
function u64ToF64(u) { _u64[0] = u; return _f64[0]; }

const stringTable = [];
const handleStore = new Map();
let nextHandleId = 1;
function allocHandle(obj) { const id = nextHandleId++; handleStore.set(id, obj); return id; }

function __bitsToJsValue(bits) {
  if (bits === TAG_UNDEFINED) return undefined;
  if (bits === TAG_NULL) return null;
  if (bits === TAG_TRUE) return true;
  if (bits === TAG_FALSE) return false;
  const tag = bits >> 48n;
  if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)];
  if (tag === POINTER_TAG) return handleStore.get(Number(bits & 0xFFFFFFFFn));
  if (tag === INT32_TAG) return Number(BigInt.asIntN(32, bits & 0xFFFFFFFFn));
  _u64[0] = bits; return _f64[0];
}
function __jsValueToBits(v) {
  if (v === undefined) return TAG_UNDEFINED;
  if (v === null) return TAG_NULL;
  if (v === true) return TAG_TRUE;
  if (v === false) return TAG_FALSE;
  if (typeof v === "number") { _f64[0] = v; return _u64[0]; }
  if (typeof v === "string") {
    const id = stringTable.length; stringTable.push(v);
    return (STRING_TAG << 48n) | BigInt(id);
  }
  if (Array.isArray(v)) {
    const id = allocHandle(v);
    return (POINTER_TAG << 48n) | BigInt(id);
  }
  if (typeof v === "object") {
    const id = allocHandle(v);
    return (POINTER_TAG << 48n) | BigInt(id);
  }
  _f64[0] = Number(v); return _u64[0];
}

let wasmInst = null;
let wasmMem = null;

self.onmessage = async function(e) {
  const msg = e.data;
  try {
    if (msg.type === "init") {
      // Build minimal imports for the WASM module
      const nan_temp = { value: 0x10000 };
      const imports = { rt: {} };
      // String init — workers need this for __init_strings
      imports.rt.string_new = (offset, len) => {
        const bytes = new Uint8Array(wasmMem.buffer, offset, len);
        stringTable.push(new TextDecoder().decode(bytes));
      };
      // Stubs for other imports — workers only call user functions via indirect table
      const noop = () => 0;
      const noopVoid = () => {};
      // Create a Proxy to provide stubs for any import the WASM module needs
      imports.rt = new Proxy(imports.rt, {
        get(target, prop) {
          if (prop in target) return target[prop];
          return noop;
        }
      });
      const module = await WebAssembly.compile(msg.binary);
      const instance = await WebAssembly.instantiate(module, imports);
      wasmInst = instance;
      wasmMem = instance.exports.memory;
      // Initialize string table
      if (instance.exports.__init_strings) instance.exports.__init_strings();
      self.postMessage({ type: "ready" });
      return;
    }

    if (!wasmInst) { self.postMessage({ type: "error", message: "Worker not initialized" }); return; }

    const table = wasmInst.exports.__indirect_function_table;
    const fn = table.get(msg.funcIdx | 0);
    if (!fn) { self.postMessage({ type: "error", message: "Function not found at index " + msg.funcIdx }); return; }

    const captures = (msg.captures || []).map(c => __jsValueToBits(c));

    if (msg.type === "map") {
      const results = [];
      for (let i = 0; i < msg.chunk.length; i++) {
        const argBits = __jsValueToBits(msg.chunk[i]);
        const resultBits = fn(...captures, argBits);
        results.push(__bitsToJsValue(resultBits));
      }
      self.postMessage({ type: "result", data: results });
    } else if (msg.type === "filter") {
      const filtered = [];
      for (let i = 0; i < msg.chunk.length; i++) {
        const argBits = __jsValueToBits(msg.chunk[i]);
        const resultBits = fn(...captures, argBits);
        const jsResult = __bitsToJsValue(resultBits);
        // Truthy check matching Perry semantics
        if (jsResult !== undefined && jsResult !== null && jsResult !== false && jsResult !== 0 && jsResult !== "") {
          filtered.push(msg.chunk[i]);
        }
      }
      self.postMessage({ type: "result", data: filtered });
    } else if (msg.type === "exec") {
      const resultBits = fn(...captures);
      self.postMessage({ type: "result", data: __bitsToJsValue(resultBits) });
    }
  } catch (err) {
    self.postMessage({ type: "error", message: String(err), stack: err && err.stack });
  }
};
`;

  function getConcurrency() {
    return (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 4;
  }

  function getWasmBinary() {
    if (wasmBinary) return wasmBinary;
    // Decode the base64 WASM that's stored on window by the HTML wrapper
    const b64 = (typeof window !== 'undefined' && window.__perryWasmB64) || null;
    if (!b64) return null;
    const raw = atob(b64);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    wasmBinary = bytes.buffer;
    return wasmBinary;
  }

  function ensureWorkers(count) {
    if (workers && workers.length >= count) return workers;
    const binary = getWasmBinary();
    if (!binary) return null;

    const blob = new Blob([WORKER_BOOTSTRAP], { type: 'text/javascript' });
    const url = URL.createObjectURL(blob);

    workers = workers || [];
    const initPromises = [];
    while (workers.length < count) {
      const w = new Worker(url);
      const ready = new Promise((resolve, reject) => {
        const handler = (e) => {
          w.removeEventListener('message', handler);
          if (e.data.type === 'ready') resolve();
          else reject(new Error(e.data.message || 'Worker init failed'));
        };
        w.addEventListener('message', handler);
      });
      w.postMessage({ type: 'init', binary });
      workers.push({ worker: w, ready });
      initPromises.push(ready);
    }
    return workers;
  }

  function dispatchToWorker(w, msg) {
    return new Promise((resolve, reject) => {
      const handler = (e) => {
        w.worker.removeEventListener('message', handler);
        if (e.data.type === 'error') reject(new Error(e.data.message));
        else resolve(e.data.data);
      };
      w.worker.addEventListener('message', handler);
      w.worker.postMessage(msg);
    });
  }

  // Convert a closure object { funcIdx, captures } to plain JS captures for the worker
  function serializeClosure(closure) {
    if (!closure || typeof closure !== 'object') return { funcIdx: 0, captures: [] };
    const captures = (closure.captures || []).map(c => {
      // captures are BigInt (NaN-boxed i64) — convert to plain JS values
      return __bitsToJsValue(typeof c === 'bigint' ? c : BigInt(c || 0));
    });
    return { funcIdx: closure.funcIdx | 0, captures };
  }

  // Sequential fallback — runs on main thread, no workers
  function mapSequential(array, closure) {
    const table = wasmInstance?.exports?.__indirect_function_table;
    if (!table) return array;
    const fn = table.get(closure.funcIdx | 0);
    if (!fn) return array;
    const captures = (closure.captures || []);
    const results = [];
    for (let i = 0; i < array.length; i++) {
      const resultBits = fn(...captures, __jsValueToBits(array[i]));
      results.push(__bitsToJsValue(resultBits));
    }
    return results;
  }

  function filterSequential(array, closure) {
    const table = wasmInstance?.exports?.__indirect_function_table;
    if (!table) return array;
    const fn = table.get(closure.funcIdx | 0);
    if (!fn) return array;
    const captures = (closure.captures || []);
    const results = [];
    for (let i = 0; i < array.length; i++) {
      const resultBits = fn(...captures, __jsValueToBits(array[i]));
      const jsResult = __bitsToJsValue(resultBits);
      if (jsResult !== undefined && jsResult !== null && jsResult !== false && jsResult !== 0 && jsResult !== "") {
        results.push(array[i]);
      }
    }
    return results;
  }

  function spawnSequential(closure) {
    const table = wasmInstance?.exports?.__indirect_function_table;
    if (!table) return undefined;
    const fn = table.get(closure.funcIdx | 0);
    if (!fn) return undefined;
    const captures = (closure.captures || []);
    return __bitsToJsValue(fn(...captures));
  }

  return {
    map(array, closure) {
      const concurrency = getConcurrency();
      if (typeof Worker === 'undefined' || array.length < MIN_PARALLEL_SIZE || concurrency <= 1 || !getWasmBinary()) {
        return mapSequential(array, closure);
      }

      const numChunks = Math.min(concurrency, array.length);
      const ws = ensureWorkers(numChunks);
      if (!ws) return mapSequential(array, closure);

      const { funcIdx, captures } = serializeClosure(closure);
      const chunkSize = Math.ceil(array.length / numChunks);
      const promises = [];
      for (let i = 0; i < numChunks; i++) {
        const chunk = array.slice(i * chunkSize, (i + 1) * chunkSize);
        if (chunk.length === 0) continue;
        const p = ws[i].ready.then(() =>
          dispatchToWorker(ws[i], { type: 'map', funcIdx, captures, chunk })
        );
        promises.push(p);
      }

      return Promise.all(promises).then(results => {
        const out = [];
        for (const r of results) for (const item of r) out.push(item);
        return out;
      });
    },

    filter(array, closure) {
      const concurrency = getConcurrency();
      if (typeof Worker === 'undefined' || array.length < MIN_PARALLEL_SIZE || concurrency <= 1 || !getWasmBinary()) {
        return filterSequential(array, closure);
      }

      const numChunks = Math.min(concurrency, array.length);
      const ws = ensureWorkers(numChunks);
      if (!ws) return filterSequential(array, closure);

      const { funcIdx, captures } = serializeClosure(closure);
      const chunkSize = Math.ceil(array.length / numChunks);
      const promises = [];
      for (let i = 0; i < numChunks; i++) {
        const chunk = array.slice(i * chunkSize, (i + 1) * chunkSize);
        if (chunk.length === 0) continue;
        const p = ws[i].ready.then(() =>
          dispatchToWorker(ws[i], { type: 'filter', funcIdx, captures, chunk })
        );
        promises.push(p);
      }

      return Promise.all(promises).then(results => {
        const out = [];
        for (const r of results) for (const item of r) out.push(item);
        return out;
      });
    },

    spawn(closure) {
      if (typeof Worker === 'undefined' || !getWasmBinary()) {
        return Promise.resolve(spawnSequential(closure));
      }

      const ws = ensureWorkers(1);
      if (!ws) return Promise.resolve(spawnSequential(closure));

      const { funcIdx, captures } = serializeClosure(closure);
      return ws[0].ready.then(() =>
        dispatchToWorker(ws[0], { type: 'exec', funcIdx, captures })
      );
    },
  };
})();

// Exception state for try/catch bridge
let tryDepth = 0;
let currentException = null;

// ===== Perry UI Runtime (DOM-based, for --target wasm) =====
// Ported from perry-codegen-js/web_runtime.js — maps perry/ui widgets to DOM elements.

const uiHandles = new Map();   // handle_id -> DOM element or state object
const uiStates = new Map();    // handle_id -> { _value, subscribers[] }
let uiNextHandle = 1;
const perryImageCache = new Map();

function uiAlloc(el) {
  const h = uiNextHandle++;
  uiHandles.set(h, el);
  return h;
}
function uiGet(h) { return uiHandles.get(h); }

// Helper: call a WASM closure — accepts either a raw NaN-boxed f64 handle,
// or a JS closure object ({funcIdx, captures}) from toJsValue conversion.
// WASM functions use i64 (BigInt) params/returns.
function callWasmClosure(closureVal, ...extraArgs) {
  let closure;
  if (typeof closureVal === 'object' && closureVal !== null) {
    // Already a JS closure object (came through toJsValue)
    closure = closureVal;
  } else if (typeof closureVal === 'number') {
    if (isUndefined(closureVal) || isNull(closureVal)) return u64ToF64(TAG_UNDEFINED);
    closure = isPointer(closureVal) ? handleStore.get(getPointerId(closureVal)) : undefined;
  }
  if (!closure) return u64ToF64(TAG_UNDEFINED);
  if (typeof closure === 'function') return fromJsValue(closure(...extraArgs.map(v => typeof v === 'number' ? v : fromJsValue(v))));
  if (typeof closure.funcIdx !== 'undefined' && wasmInstance) {
    const fn = wasmInstance.exports.__indirect_function_table.get(closure.funcIdx | 0);
    // Extra args need to be BigInt (i64) for WASM functions. Captures are already BigInt.
    const wasmArgs = extraArgs.map(v => __jsValueToBits(v));
    if (fn) {
      const result = fn(...(closure.captures || []), ...wasmArgs);
      return (typeof result === 'bigint') ? __bitsToJsValue(result) : undefined;
    }
  }
  return u64ToF64(TAG_UNDEFINED);
}

// CSS Reset for UI
(function() {
  const s = document.createElement("style");
  s.textContent = `
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
html, body { width: 100vw; height: 100vh; overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
#perry-root { display: flex; flex-direction: column; width: 100%; flex: 1 1 0%; min-height: 0; overflow: hidden; }
button { cursor: pointer; padding: 6px 16px; border: 1px solid #ccc; border-radius: 6px; background: transparent; font: inherit; color: inherit; }
button:hover { opacity: 0.85; }
button:active { opacity: 0.7; }
input[type="text"], input[type="password"], select, textarea { padding: 6px 10px; border: 1px solid #ccc; border-radius: 6px; font: inherit; }
input[type="range"] { width: 100%; }
hr { border: none; border-top: 1px solid #ddd; margin: 4px 0; }
fieldset { border: 1px solid #ddd; border-radius: 8px; padding: 12px; }
legend { font-weight: 600; padding: 0 6px; }
progress { width: 100%; }
  `;
  document.head.appendChild(s);
})();

function uiGetRoot() {
  let r = document.getElementById("perry-root");
  if (!r) { r = document.createElement("div"); r.id = "perry-root"; document.body.appendChild(r); }
  return r;
}

// State system
function uiStateCreate(initial) {
  const h = uiNextHandle++;
  uiStates.set(h, { _value: initial, subscribers: [] });
  return h;
}
function uiStateGet(h) { const s = uiStates.get(h); return s ? s._value : undefined; }
function uiStateSet(h, value) {
  const s = uiStates.get(h);
  if (!s) return;
  s._value = value;
  const subs = s.subscribers.slice();
  for (const sub of subs) { try { sub(value); } catch(e) { console.error("State subscriber error:", e); } }
}

// ---------- Widget creation functions (take JS values, return handle_id) ----------

// #1546: showToast(msg) — bottom-center fade-in/out toast for --target web.
// HarmonyOS routes through `promptAction.showToast` via a runtime drain queue;
// on web the previous behavior was a documented no-op. The implementation here
// stays minimal: one host-managed div, queued messages, default 3s visible.
function perry_ui_show_toast(message) {
  if (typeof document === "undefined") return;
  const msg = (message === null || message === undefined) ? "" : String(message);
  if (!perry_ui_show_toast._q) perry_ui_show_toast._q = [];
  perry_ui_show_toast._q.push(msg);
  if (perry_ui_show_toast._busy) return;
  perry_ui_show_toast._busy = true;
  const drain = () => {
    const next = perry_ui_show_toast._q.shift();
    if (next === undefined) { perry_ui_show_toast._busy = false; return; }
    let el = perry_ui_show_toast._el;
    if (!el) {
      el = document.createElement("div");
      el.setAttribute("data-perry-toast", "");
      el.style.cssText =
        "position:fixed;left:50%;bottom:32px;transform:translateX(-50%);" +
        "background:rgba(28,28,30,0.92);color:#fff;padding:10px 18px;" +
        "border-radius:18px;font:14px -apple-system,system-ui,Segoe UI,Roboto,sans-serif;" +
        "max-width:80vw;box-shadow:0 6px 24px rgba(0,0,0,0.25);" +
        "opacity:0;transition:opacity .18s ease;pointer-events:none;z-index:2147483647;";
      document.body.appendChild(el);
      perry_ui_show_toast._el = el;
    }
    el.textContent = next;
    requestAnimationFrame(() => { el.style.opacity = "1"; });
    setTimeout(() => {
      el.style.opacity = "0";
      setTimeout(drain, 200);
    }, 3000);
  };
  drain();
}

function perry_ui_app_create(titleOrOpts, width, height) {
  let title = "Perry App", bodyH, w = 800, ht = 600;
  if (typeof titleOrOpts === "object" && titleOrOpts !== null) {
    title = titleOrOpts.title || title; w = titleOrOpts.width || w; ht = titleOrOpts.height || ht;
    bodyH = titleOrOpts.body;
  } else { title = titleOrOpts || title; w = width || w; ht = height || ht; }
  document.title = title;
  const root = uiGetRoot();
  root.style.width = w + "px"; root.style.maxWidth = "100%"; root.style.height = ht + "px"; root.style.maxHeight = "100%";
  const h = uiAlloc(root);
  if (bodyH !== undefined) { const bodyEl = uiGet(bodyH); if (bodyEl) root.appendChild(bodyEl); }
  return h;
}
function perry_ui_vstack_create(spacing, children) {
  const el = document.createElement("div");
  el.style.display = "flex"; el.style.flexDirection = "column";
  if (spacing) el.style.gap = spacing + "px";
  el.style.flex = "1 1 0%"; el.style.minHeight = "0"; el.style.overflow = "auto";
  const h = uiAlloc(el);
  if (Array.isArray(children)) { for (var i = 0; i < children.length; i++) { var c = uiGet(children[i]); if (c) el.appendChild(c); } }
  return h;
}
function perry_ui_hstack_create(spacing, children) {
  const el = document.createElement("div");
  el.style.display = "flex"; el.style.flexDirection = "row"; el.style.alignItems = "center";
  if (spacing) el.style.gap = spacing + "px";
  const h = uiAlloc(el);
  if (Array.isArray(children)) { for (var i = 0; i < children.length; i++) { var c = uiGet(children[i]); if (c) el.appendChild(c); } }
  return h;
}
function perry_ui_zstack_create() {
  const el = document.createElement("div");
  el.style.position = "relative"; el.style.display = "flex"; el.style.flex = "1 1 0%";
  return uiAlloc(el);
}
function perry_ui_text_create(text, id) {
  const el = document.createElement("span");
  el.textContent = (text !== undefined && text !== null) ? String(text) : "";
  const h = uiAlloc(el);
  // Issue #1392 — 2-arg `Text(content, id)` form. The state_desugar pass
  // emits `count.text()` as `Text("<initial>", "__state_N")`; the id string
  // is reused as the keyed-state key, so registering the widget here lets
  // `setText(id, …)` and `__state_set("__state_N", …)` find and re-render it.
  if (typeof id === "string" && id.length > 0) uiTextRegister(h, id);
  return h;
}
function perry_ui_button_create(label, callback) {
  const el = document.createElement("button");
  el.textContent = (typeof label === 'string') ? label : String(label ?? "");
  el._perryCallback = callback;
  el.addEventListener("click", () => { if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback); });
  return uiAlloc(el);
}
function perry_ui_textfield_create(placeholder, callback) {
  const el = document.createElement("input"); el.type = "text";
  el.placeholder = placeholder || "";
  el._perryCallback = callback;
  el.addEventListener("input", () => {
    if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback, fromJsValue(el.value));
  });
  return uiAlloc(el);
}
function perry_ui_securefield_create(placeholder, callback) {
  const el = document.createElement("input"); el.type = "password";
  el.placeholder = placeholder || "";
  el._perryCallback = callback;
  el.addEventListener("input", () => {
    if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback, fromJsValue(el.value));
  });
  return uiAlloc(el);
}
function perry_ui_toggle_create(label, callback) {
  const wrap = document.createElement("label");
  wrap.style.display = "flex"; wrap.style.alignItems = "center"; wrap.style.gap = "8px";
  const inp = document.createElement("input"); inp.type = "checkbox";
  wrap.appendChild(inp);
  if (label) { const sp = document.createElement("span"); sp.textContent = label; wrap.appendChild(sp); }
  wrap._perryCallback = callback;
  wrap._inp = inp;
  inp.addEventListener("change", () => {
    if (wrap._perryCallback !== undefined) callWasmClosure(wrap._perryCallback, inp.checked ? 1.0 : 0.0);
  });
  return uiAlloc(wrap);
}
function perry_ui_slider_create(min, max, callback) {
  // Codegen emits 3-arg Slider(min, max, onChange); default initial=min
  const el = document.createElement("input"); el.type = "range";
  el.min = min || 0; el.max = max || 100; el.value = el.min; el.step = "any";
  el._perryCallback = callback;
  el.addEventListener("input", () => {
    if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback, parseFloat(el.value));
  });
  return uiAlloc(el);
}
function perry_ui_scrollview_create() {
  const el = document.createElement("div");
  el.style.overflow = "auto"; el.style.flex = "1 1 0%"; el.style.minHeight = "0";
  return uiAlloc(el);
}
function perry_ui_spacer_create() {
  const el = document.createElement("div"); el.style.flex = "1 1 0%"; return uiAlloc(el);
}
function perry_ui_divider_create() { return uiAlloc(document.createElement("hr")); }
function perry_ui_progressview_create(value) {
  const el = document.createElement("progress");
  if (value !== undefined && value > 0) { el.max = 1; el.value = value; }
  return uiAlloc(el);
}
function perry_ui_image_create(src, width, height) {
  const el = document.createElement("img");
  if (src) el.src = src;
  if (width) el.style.width = width + "px";
  if (height) el.style.height = height + "px";
  return uiAlloc(el);
}
function perry_ui_picker_create(items_json, selected, callback) {
  const el = document.createElement("select");
  el._perryCallback = callback;
  el.addEventListener("change", () => {
    if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback, el.selectedIndex);
  });
  if (selected !== undefined) el.selectedIndex = selected;
  return uiAlloc(el);
}
function perry_ui_form_create() {
  const el = document.createElement("fieldset");
  el.style.display = "flex"; el.style.flexDirection = "column"; el.style.gap = "8px";
  return uiAlloc(el);
}
function perry_ui_section_create(title) {
  const el = document.createElement("fieldset");
  el.style.display = "flex"; el.style.flexDirection = "column"; el.style.gap = "8px";
  if (title) { const lg = document.createElement("legend"); lg.textContent = title; el.appendChild(lg); }
  return uiAlloc(el);
}
function perry_ui_navigationstack_create() {
  const el = document.createElement("div");
  el.style.display = "flex"; el.style.flexDirection = "column"; el.style.flex = "1 1 0%";
  el._navStack = [];
  return uiAlloc(el);
}
function perry_ui_canvas_create(width, height) {
  const el = document.createElement("canvas");
  el.width = width || 300; el.height = height || 150;
  el._ctx = el.getContext("2d");
  return uiAlloc(el);
}
// BloomView (issue #2395) — a <canvas> render-surface host for an external GPU
// renderer. The web has no HWND; bloomViewGetHwnd returns the handle id (the
// surrogate the renderer keys off of) or 0 if unknown.
function perry_ui_bloomview_create(width, height) {
  const el = document.createElement("canvas");
  el.width = width || 300; el.height = height || 150;
  return uiAlloc(el);
}
function perry_ui_bloomview_get_hwnd(h) {
  return uiGet(h) ? h : 0;
}
function perry_ui_lazyvstack_create(count, renderClosure) {
  const el = document.createElement("div");
  el.style.display = "flex"; el.style.flexDirection = "column"; el.style.overflow = "auto"; el.style.flex = "1 1 0%";
  el._renderClosure = renderClosure; el._count = count;
  for (let i = 0; i < count; i++) {
    const childVal = callWasmClosure(renderClosure, i);
    const childEl = uiGet(typeof childVal === 'number' ? childVal : 0);
    if (childEl) el.appendChild(childEl);
  }
  return uiAlloc(el);
}
function perry_ui_lazyvstack_update(h, count) {
  const el = uiGet(h); if (!el) return;
  el.innerHTML = ''; el._count = count;
  for (let i = 0; i < count; i++) {
    const childVal = callWasmClosure(el._renderClosure, i);
    const childEl = uiGet(typeof childVal === 'number' ? childVal : 0);
    if (childEl) el.appendChild(childEl);
  }
}
function perry_ui_table_create(rowCount, colCount, renderClosure) {
  const table = document.createElement("table");
  table.style.width = "100%"; table.style.borderCollapse = "collapse";
  table._renderClosure = renderClosure; table._rowCount = rowCount; table._colCount = colCount;
  table._selectedRow = -1;
  const thead = document.createElement("thead"); table.appendChild(thead);
  const headerRow = document.createElement("tr"); thead.appendChild(headerRow);
  for (let c = 0; c < colCount; c++) { const th = document.createElement("th"); th.textContent = "Col " + c; headerRow.appendChild(th); }
  const tbody = document.createElement("tbody"); table.appendChild(tbody);
  for (let r = 0; r < rowCount; r++) {
    const tr = document.createElement("tr"); tr.style.cursor = "pointer";
    tr.addEventListener("click", () => { table._selectedRow = r; if (table._onRowSelect) callWasmClosure(table._onRowSelect, r); });
    for (let c = 0; c < colCount; c++) {
      const td = document.createElement("td"); td.style.padding = "4px 8px"; td.style.borderBottom = "1px solid #eee";
      const cellVal = callWasmClosure(renderClosure, r, c);
      const cellEl = uiGet(typeof cellVal === 'number' ? cellVal : 0);
      if (cellEl) td.appendChild(cellEl); else td.textContent = String(toJsValue(cellVal) ?? "");
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }
  return uiAlloc(table);
}
function perry_ui_table_set_column_header(h, col, title) {
  const t = uiGet(h); if (!t) return; const th = t.querySelector("thead tr")?.children[col]; if (th) th.textContent = title;
}
function perry_ui_table_set_column_width(h, col, width) {
  const t = uiGet(h); if (!t) return; const th = t.querySelector("thead tr")?.children[col]; if (th) th.style.width = width + "px";
}
function perry_ui_table_update_row_count(h, count) { /* simplified: just update count */ }
function perry_ui_table_set_on_row_select(h, cb) { const t = uiGet(h); if (t) t._onRowSelect = cb; }
function perry_ui_table_get_selected_row(h) { const t = uiGet(h); return t ? t._selectedRow : -1; }
function perry_ui_textarea_create(placeholder, callback) {
  const el = document.createElement("textarea");
  el.placeholder = placeholder || "";
  el._perryCallback = callback;
  el.addEventListener("input", () => {
    if (el._perryCallback !== undefined) callWasmClosure(el._perryCallback, fromJsValue(el.value));
  });
  return uiAlloc(el);
}
function perry_ui_textarea_set_string(h, text) { const el = uiGet(h); if (el) el.value = text; }
function perry_ui_textarea_get_string(h) { const el = uiGet(h); return el ? el.value : ""; }

// ---------- Child management ----------
function perry_ui_widget_add_child(parentH, childH) {
  const p = uiGet(parentH), c = uiGet(childH);
  if (p && c) p.appendChild(c);
}
function perry_ui_widget_remove_all_children(h) { const el = uiGet(h); if (el) el.innerHTML = ""; }
function perry_ui_widget_remove_child(parentH, childH) {
  const p = uiGet(parentH), c = uiGet(childH);
  if (p && c && p.contains(c)) p.removeChild(c);
}
function perry_ui_widget_reorder_child(parentH, fromIdx, toIdx) {
  const p = uiGet(parentH); if (!p) return;
  const children = Array.from(p.children);
  if (fromIdx >= 0 && fromIdx < children.length) {
    const child = children[fromIdx]; p.removeChild(child);
    if (toIdx >= p.children.length) p.appendChild(child);
    else p.insertBefore(child, p.children[toIdx]);
  }
}
function perry_ui_widget_add_overlay(parentH, overlayH) {
  const p = uiGet(parentH), o = uiGet(overlayH);
  if (p && o) { o.style.position = "absolute"; p.appendChild(o); }
}
function perry_ui_widget_set_overlay_frame(overlayH, x, y, width, height) {
  const o = uiGet(overlayH); if (!o) return;
  o.style.left = x + "px"; o.style.top = y + "px"; o.style.width = width + "px"; o.style.height = height + "px";
}

// ---------- Styling ----------
function perry_ui_set_background(h, r, g, b, a) {
  const el = uiGet(h); if (el) el.style.backgroundColor = `rgba(${r*255|0},${g*255|0},${b*255|0},${a})`;
}
function perry_ui_set_foreground(h, r, g, b, a) {
  const el = uiGet(h); if (el) el.style.color = `rgba(${r*255|0},${g*255|0},${b*255|0},${a})`;
}
function perry_ui_set_font_size(h, size) { const el = uiGet(h); if (el) el.style.fontSize = size + "px"; }
function perry_ui_set_font_weight(h, weight) { const el = uiGet(h); if (el) el.style.fontWeight = weight; }
function perry_ui_set_font_family(h, family) { const el = uiGet(h); if (el) el.style.fontFamily = family; }
function perry_ui_set_padding(h, value) { const el = uiGet(h); if (el) el.style.padding = value + "px"; }
function perry_ui_set_frame(h, width, height) {
  const el = uiGet(h); if (!el) return;
  if (width > 0) el.style.width = width + "px";
  if (height > 0) el.style.height = height + "px";
}
function perry_ui_set_corner_radius(h, radius) { const el = uiGet(h); if (el) el.style.borderRadius = radius + "px"; }
function perry_ui_set_border(h, width, r, g, b, a) {
  const el = uiGet(h); if (el) el.style.border = `${width}px solid rgba(${r*255|0},${g*255|0},${b*255|0},${a})`;
}
// Joint border state for the Apple-style split setters
// (`widgetSetBorderColor` / `widgetSetBorderWidth`). CSS won't render a
// border unless style + color + width are all set in the same rule, so
// we cache (color, width) per handle and re-apply `el.style.border` on
// every change. Defaults match CALayer-ish behavior: missing color =
// black, missing width = 1px. Issue #185 Phase B closure.
const __perryBorderState = new Map();
function __perry_apply_border(h) {
  const el = uiGet(h);
  if (!el) return;
  const s = __perryBorderState.get(h) || {};
  const color = s.color || [0, 0, 0, 1];
  const width = s.width != null ? s.width : 1;
  const [r, g, b, a] = color;
  el.style.border = `${width}px solid rgba(${r*255|0},${g*255|0},${b*255|0},${a})`;
}
function perry_ui_widget_set_border_color(h, r, g, b, a) {
  const s = __perryBorderState.get(h) || {};
  s.color = [r, g, b, a];
  __perryBorderState.set(h, s);
  __perry_apply_border(h);
}
function perry_ui_widget_set_border_width(h, width) {
  const s = __perryBorderState.get(h) || {};
  s.width = width;
  __perryBorderState.set(h, s);
  __perry_apply_border(h);
}
// Text decoration (issue #185 Phase B closure). 0=none, 1=underline,
// 2=strikethrough. CSS `text-decoration` values map cleanly.
function perry_ui_text_set_decoration(h, decoration) {
  const el = uiGet(h);
  if (!el) return;
  const css = decoration === 1 ? "underline"
    : decoration === 2 ? "line-through"
    : "none";
  el.style.textDecoration = css;
}
// Issue #3621 — horizontal text alignment. Canonical Perry/AppKit values:
// 0=left, 1=right, 2=center, 3=justified, 4=natural. Maps to CSS
// `text-align` (natural → `start`, which follows the element's direction).
function perry_ui_text_set_text_alignment(h, alignment) {
  const el = uiGet(h);
  if (!el) return;
  const css = alignment === 1 ? "right"
    : alignment === 2 ? "center"
    : alignment === 3 ? "justify"
    : alignment === 4 ? "start"
    : "left";
  el.style.textAlign = css;
}
// Issue #185 Phase B closure 11 — TextField borderless. Drops the
// rendered border so the input visually matches a non-bordered
// borderless setter on the Apple side.
function perry_ui_textfield_set_borderless(h, borderless) {
  const el = uiGet(h);
  if (!el) return;
  el.style.border = borderless ? "none" : "";
  // Also strip the default browser focus outline so "borderless"
  // really feels borderless. User can override with explicit
  // border / outline rules afterwards.
  el.style.outline = borderless ? "none" : "";
}
// Issue #185 Phase B closure 11 — Stack alignment via CSS flex.
// Numeric enum: 0=fill, 1=center, 2=leading, 3=trailing, 4=baseline.
// Maps to CSS `align-items` (cross-axis on flex containers).
function perry_ui_stack_set_alignment(h, alignment) {
  const el = uiGet(h);
  if (!el) return;
  const value = alignment === 0 ? "stretch"
    : alignment === 1 ? "center"
    : alignment === 2 ? "flex-start"
    : alignment === 3 ? "flex-end"
    : alignment === 4 ? "baseline"
    : "stretch";
  el.style.alignItems = value;
}
// Drop shadow (issue #185 Phase B closure 2). Same arg shape as the
// Apple CALayer twin — `(r,g,b,a)` is shadow color, `blur` is shadow
// radius, `(offsetX, offsetY)` is shadow offset (positive y = downward,
// matching CALayer + native HTML semantics).
function perry_ui_widget_set_shadow(h, r, g, b, a, blur, offsetX, offsetY) {
  const el = uiGet(h);
  if (!el) return;
  el.style.boxShadow = `${offsetX}px ${offsetY}px ${blur}px rgba(${r*255|0},${g*255|0},${b*255|0},${a})`;
}
function perry_ui_set_opacity(h, opacity) { const el = uiGet(h); if (el) el.style.opacity = opacity; }
function perry_ui_set_enabled(h, enabled) {
  const el = uiGet(h); if (el) { el.disabled = !enabled; el.style.pointerEvents = enabled ? "" : "none"; el.style.opacity = enabled ? "" : "0.5"; }
}
function perry_ui_set_tooltip(h, text) { const el = uiGet(h); if (el) el.title = text || ""; }
function perry_ui_set_control_size(h, size) { /* 0=regular 1=small 2=mini 3=large */
  const el = uiGet(h); if (!el) return;
  const sizes = ["", "0.85em", "0.75em", "1.2em"];
  el.style.fontSize = sizes[size] || "";
}
function perry_ui_set_widget_hidden(h, hidden) { const el = uiGet(h); if (el) el.style.display = hidden ? "none" : ""; }
function perry_ui_widget_set_background_gradient(h, r1, g1, b1, a1, r2, g2, b2, a2, direction) {
  const el = uiGet(h); if (!el) return;
  const c1 = `rgba(${r1*255|0},${g1*255|0},${b1*255|0},${a1})`, c2 = `rgba(${r2*255|0},${g2*255|0},${b2*255|0},${a2})`;
  const dir = direction === 1 ? "to right" : "to bottom";
  el.style.background = `linear-gradient(${dir}, ${c1}, ${c2})`;
}
function perry_ui_widget_set_width(h, w) { const el = uiGet(h); if (el) el.style.width = w + "px"; }
function perry_ui_widget_set_height(h, height) { const el = uiGet(h); if (el) el.style.height = height + "px"; }
function perry_ui_widget_set_hugging(h) { const el = uiGet(h); if (el) el.style.flex = "0 0 auto"; }
function perry_ui_widget_match_parent_width(h) { const el = uiGet(h); if (el) el.style.width = "100%"; }
function perry_ui_widget_match_parent_height(h) { const el = uiGet(h); if (el) el.style.height = "100%"; }
function perry_ui_widget_set_edge_insets(h, top, right, bottom, left) {
  const el = uiGet(h); if (el) el.style.padding = `${top}px ${right}px ${bottom}px ${left}px`;
}
function perry_ui_stack_set_detaches_hidden(h) { /* no-op in web */ }
function perry_ui_stack_set_distribution(h) { /* no-op in web */ }
function perry_ui_widget_set_context_menu(widgetH, menuH) { /* simplified stub */ }

// ---------- Animations ----------
// Canonical API: animateOpacity(target, durationSecs), animatePosition(dx, dy, durationSecs).
function perry_ui_animate_opacity(h, target, durationSecs) {
  const el = uiGet(h); if (!el) return;
  const cur = el.style.opacity === "" ? "1" : el.style.opacity;
  el.style.opacity = cur;
  el.style.transition = `opacity ${durationSecs}s ease`;
  requestAnimationFrame(() => { el.style.opacity = target; });
}
function perry_ui_animate_position(h, dx, dy, durationSecs) {
  const el = uiGet(h); if (!el) return;
  const m = /translate\(([-\d.]+)px,\s*([-\d.]+)px\)/.exec(el.style.transform || "");
  const curX = m ? parseFloat(m[1]) : 0;
  const curY = m ? parseFloat(m[2]) : 0;
  el.style.transform = `translate(${curX}px, ${curY}px)`;
  el.style.transition = `transform ${durationSecs}s ease`;
  requestAnimationFrame(() => { el.style.transform = `translate(${curX + dx}px, ${curY + dy}px)`; });
}

// ---------- Events ----------
// Issue #1865: display-link callbacks. One-shot per registration, like
// requestAnimationFrame. The id we return matches the rAF id so cancelFrame
// can hand it straight to cancelAnimationFrame. timestampMs is rAF's
// DOMHighResTimeStamp; deltaMs is computed per stable WASM function-table
// index because the JS closure wrapper crossing the WASM boundary is freshly
// allocated on each call. This keeps the idiomatic
// `function loop(t,dt){ ...; onFrame(loop); }` pattern accurate.
const __perryFrameLastByFuncIdx = new Map();
function perry_ui_on_frame(callback) {
  if (!callback || typeof callback.funcIdx === 'undefined') return 0;
  const key = callback.funcIdx | 0;
  const id = requestAnimationFrame((ts) => {
    const prev = __perryFrameLastByFuncIdx.get(key);
    const dt = (typeof prev === 'number' && ts >= prev) ? ts - prev : 0;
    __perryFrameLastByFuncIdx.set(key, ts);
    callWasmClosure(callback, ts, dt);
  });
  return id;
}
function perry_ui_cancel_frame(id) { cancelAnimationFrame(id); }

function perry_ui_set_on_click(h, callback) {
  const el = uiGet(h); if (el) el.addEventListener("click", () => callWasmClosure(callback));
}
function perry_ui_set_on_hover(h, callback) {
  const el = uiGet(h); if (!el) return;
  el.addEventListener("mouseenter", () => callWasmClosure(callback, 1.0));
  el.addEventListener("mouseleave", () => callWasmClosure(callback, 0.0));
}
function perry_ui_set_on_double_click(h, callback) {
  const el = uiGet(h); if (el) el.addEventListener("dblclick", () => callWasmClosure(callback));
}

// ---------- State ----------
function perry_ui_state_create(initial) { return uiStateCreate(initial); }
function perry_ui_state_get(h) { return uiStateGet(h); }
function perry_ui_state_set(h, v) { uiStateSet(h, v); }
function perry_ui_state_on_change(stateH, callback) {
  const s = uiStates.get(stateH);
  if (s) s.subscribers.push((val) => callWasmClosure(callback, fromJsValue(val)));
}
function perry_ui_state_bind_text(stateH, widgetH) {
  const el = uiGet(widgetH), s = uiStates.get(stateH);
  if (el && s) { el.textContent = String(s._value ?? ""); s.subscribers.push(v => { el.textContent = String(v ?? ""); }); }
}
function perry_ui_state_bind_text_numeric(stateH, widgetH) { perry_ui_state_bind_text(stateH, widgetH); }
function perry_ui_state_bind_slider(stateH, widgetH) {
  const el = uiGet(widgetH), s = uiStates.get(stateH);
  if (el && s) {
    el.value = s._value; s.subscribers.push(v => { el.value = v; });
    el.addEventListener("input", () => uiStateSet(stateH, parseFloat(el.value)));
  }
}
function perry_ui_state_bind_toggle(stateH, widgetH) {
  const wrap = uiGet(widgetH), s = uiStates.get(stateH);
  if (wrap && s) {
    const inp = wrap._inp || wrap.querySelector("input");
    if (inp) { inp.checked = !!s._value; s.subscribers.push(v => { inp.checked = !!v; }); }
  }
}
// Issue #5076: programmatically set a Toggle's on/off state (0 = off, non-zero = on).
function perry_ui_toggle_set_state(widgetH, on) {
  const wrap = uiGet(widgetH);
  if (!wrap) return;
  const inp = wrap._inp || wrap.querySelector("input");
  if (inp) inp.checked = !!on;
}
function perry_ui_state_bind_visibility(stateH, widgetH) {
  const el = uiGet(widgetH), s = uiStates.get(stateH);
  if (el && s) {
    el.style.display = s._value ? "" : "none";
    s.subscribers.push(v => { el.style.display = v ? "" : "none"; });
  }
}
function perry_ui_state_bind_foreach(stateH, parentH, templateClosure) {
  const s = uiStates.get(stateH);
  if (!s) return;
  function rebuild(val) {
    const p = uiGet(parentH); if (!p) return;
    p.innerHTML = "";
    const count = typeof val === 'number' ? val : (Array.isArray(val) ? val.length : 0);
    for (let i = 0; i < count; i++) {
      const childVal = callWasmClosure(templateClosure, i);
      const childEl = uiGet(typeof childVal === 'number' ? childVal : 0);
      if (childEl) p.appendChild(childEl);
    }
  }
  rebuild(s._value);
  s.subscribers.push(rebuild);
}
function perry_ui_state_bind_textfield(stateH, widgetH) {
  const el = uiGet(widgetH), s = uiStates.get(stateH);
  if (el && s) {
    el.value = String(s._value ?? "");
    s.subscribers.push(v => { el.value = String(v ?? ""); });
    el.addEventListener("input", () => uiStateSet(stateH, el.value));
  }
}

// ---------- Issue #1392: keyed state + setText registry ----------
// The state_desugar pass (crates/perry-transform/src/state_desugar.rs) lowers
// `state<T>` to a synthetic, string-keyed registry API rather than the
// handle-based `perry_ui_state_create` surface above:
//   let c = state(0)      -> __state_init("__state_0", 0)
//   c.value / c.get()     -> __state_get("__state_0")
//   c.set(v)              -> __state_set("__state_0", v)
//   c.text()              -> Text("0", "__state_0")   (registers via uiTextRegister)
// The synth id ("__state_0") is reused as both the state key and the Text
// widget's setText id, so a write fans out to every bound widget. This
// mirrors the native runtime (perry-runtime/src/ui_text_registry.rs:
// js_state_init / js_state_get / js_state_set + perry_arkts_set_text).
const keyedStates = new Map();    // synth id -> current value
const uiTextIds = new Map();      // text id  -> [widget handle, ...]
const keyedNavstack = new Map();  // synth id -> [{ name, handle }, ...]
const keyedForeach = new Map();   // synth id -> [{ host, render }, ...]

function uiTextRegister(handle, id) {
  let list = uiTextIds.get(id);
  if (!list) { list = []; uiTextIds.set(id, list); }
  list.push(handle);
}

function perry_ui_set_text(id, value) {
  const list = uiTextIds.get(id);
  if (!list) return;
  const str = (value === undefined || value === null) ? "" : String(value);
  for (const h of list) { const el = uiGet(h); if (el) el.textContent = str; }
}

function perry_ui_state_init(id, initial) { keyedStates.set(id, initial); }
function perry_ui_state_get_keyed(id) {
  return keyedStates.has(id) ? keyedStates.get(id) : undefined;
}
function perry_ui_state_set_keyed(id, value) {
  keyedStates.set(id, value);
  perry_ui_set_text(id, value);          // reactive Text re-render
  navstackDispatchKeyed(id, value);      // route visibility
  foreachDispatchKeyed(id, value);       // dynamic-list re-render
}

function foreachRenderKeyed(host, render, value) {
  const p = uiGet(host); if (!p) return;
  p.innerHTML = "";
  const count = typeof value === 'number' ? value : (Array.isArray(value) ? value.length : 0);
  for (let i = 0; i < count; i++) {
    const childVal = callWasmClosure(render, i);
    const childEl = uiGet(typeof childVal === 'number' ? childVal : 0);
    if (childEl) p.appendChild(childEl);
  }
}
function foreachDispatchKeyed(id, value) {
  const binds = keyedForeach.get(id); if (!binds) return;
  for (const b of binds) foreachRenderKeyed(b.host, b.render, value);
}
function perry_ui_foreach_register(id, host, render) {
  let list = keyedForeach.get(id);
  if (!list) { list = []; keyedForeach.set(id, list); }
  list.push({ host, render });
  if (keyedStates.has(id)) foreachRenderKeyed(host, render, keyedStates.get(id));
}

function navstackDispatchKeyed(id, value) {
  const routes = keyedNavstack.get(id); if (!routes) return;
  const cur = (value === undefined || value === null) ? "" : String(value);
  for (const r of routes) { const el = uiGet(r.handle); if (el) el.style.display = (r.name === cur) ? "" : "none"; }
}
function perry_ui_navstack_register_route(id, name, body) {
  let list = keyedNavstack.get(id);
  if (!list) { list = []; keyedNavstack.set(id, list); }
  list.push({ name, handle: body });
  if (keyedStates.has(id)) {
    const cur = String(keyedStates.get(id));
    const el = uiGet(body); if (el && name !== cur) el.style.display = "none";
  }
}

// ---------- Text/Button/TextField ops ----------
function perry_ui_text_set_string(h, text) { const el = uiGet(h); if (el) el.textContent = String(text ?? ""); }
function perry_ui_text_set_selectable(h, selectable) { const el = uiGet(h); if (el) el.style.userSelect = selectable ? "text" : "none"; }
function perry_ui_text_set_wraps(h) { const el = uiGet(h); if (el) el.style.wordWrap = "break-word"; }
function perry_ui_text_set_color(h, r, g, b, a) { perry_ui_set_foreground(h, r, g, b, a); }
function perry_ui_button_set_bordered(h, bordered) { const el = uiGet(h); if (el) el.style.border = bordered ? "" : "none"; }
function perry_ui_button_set_title(h, title) { const el = uiGet(h); if (el) el.textContent = title; }
function perry_ui_button_set_text_color(h, r, g, b, a) { perry_ui_set_foreground(h, r, g, b, a); }
function perry_ui_button_set_image(h, name) { /* SF symbols not available in web */ }
function perry_ui_button_set_content_tint_color(h, r, g, b, a) { perry_ui_set_foreground(h, r, g, b, a); }
function perry_ui_button_set_image_position() { /* no-op in web */ }
function perry_ui_textfield_focus(h) { const el = uiGet(h); if (el) el.focus(); }
function perry_ui_textfield_set_string(h, text) { const el = uiGet(h); if (el) el.value = String(text ?? ""); }
function perry_ui_textfield_get_string(h) { const el = uiGet(h); return el ? el.value : ""; }
function perry_ui_textfield_blur_all() { if (document.activeElement) document.activeElement.blur(); }
function perry_ui_textfield_set_on_submit(h, callback) {
  const el = uiGet(h); if (el) el.addEventListener("keydown", (e) => { if (e.key === "Enter") callWasmClosure(callback); });
}
function perry_ui_textfield_set_on_focus(h, callback) {
  const el = uiGet(h); if (el) el.addEventListener("focus", () => callWasmClosure(callback));
}

// ---------- ScrollView ----------
function perry_ui_scrollview_set_child(scrollH, childH) {
  const s = uiGet(scrollH), c = uiGet(childH);
  if (s && c) { s.innerHTML = ""; s.appendChild(c); }
}
function perry_ui_scrollview_scroll_to(scrollH, childH) {
  const c = uiGet(childH); if (c) c.scrollIntoView({ behavior: "smooth" });
}
function perry_ui_scrollview_get_offset(scrollH) { const s = uiGet(scrollH); return s ? s.scrollTop : 0; }
function perry_ui_scrollview_set_offset(scrollH, offset) { const s = uiGet(scrollH); if (s) s.scrollTop = offset; }

// ---------- Layout ----------
function perry_ui_vstack_create_with_insets(spacing, top, left, bottom, right) {
  const h = perry_ui_vstack_create(spacing);
  const el = uiGet(h); if (el) el.style.padding = `${top}px ${right}px ${bottom}px ${left}px`;
  return h;
}
function perry_ui_hstack_create_with_insets(spacing, top, left, bottom, right) {
  const h = perry_ui_hstack_create(spacing);
  const el = uiGet(h); if (el) el.style.padding = `${top}px ${right}px ${bottom}px ${left}px`;
  return h;
}

// ---------- Navigation ----------
function perry_ui_navstack_push(h, bodyH) {
  const nav = uiGet(h), body = uiGet(bodyH);
  if (nav && body) { nav._navStack = nav._navStack || []; nav._navStack.push(Array.from(nav.children)); nav.innerHTML = ""; nav.appendChild(body); }
}
function perry_ui_navstack_pop(h) {
  const nav = uiGet(h); if (!nav || !nav._navStack?.length) return;
  const prev = nav._navStack.pop(); nav.innerHTML = ""; prev.forEach(c => nav.appendChild(c));
}

// ---------- Picker ----------
function perry_ui_picker_add_item(h, title) {
  const el = uiGet(h); if (!el) return;
  const opt = document.createElement("option"); opt.textContent = title; el.appendChild(opt);
}
function perry_ui_picker_set_selected(h, index) { const el = uiGet(h); if (el) el.selectedIndex = index; }
function perry_ui_picker_get_selected(h) { const el = uiGet(h); return el ? el.selectedIndex : -1; }

// ---------- Image ----------
function perry_ui_image_create_symbol(name) { return perry_ui_text_create("⬜ " + name); }
function perry_ui_image_create_url(url, alt) {
  const el = document.createElement("img");
  el.src = url || "";
  if (alt) el.alt = alt;
  el.style.objectFit = "cover";
  return uiAlloc(el);
}
function perry_ui_image_set_size(h, width, height) {
  const el = uiGet(h); if (!el) return;
  if (width) el.style.width = width + "px"; if (height) el.style.height = height + "px";
}
function perry_ui_image_set_tint(h, r, g, b, a) { perry_ui_set_foreground(h, r, g, b, a); }

// ---------- WebView (issue #658 Phase 5 web target) ----------
// Renders an <iframe>. Cross-origin restrictions limit what we can hook:
// `onShouldNavigate` only fires for same-origin nav (we observe via the
// `load` event and inspect contentWindow.location); `onLoaded` fires
// after the iframe's `load` event for any nav; `onError` is delivered
// when the iframe fails to load (same-origin only). Custom UA is
// browser-controlled and can't be overridden from JS.
//
// v2-C cross-origin postMessage bridge: when a `postMessage` arrives
// from the iframe's contentWindow, we route it to the per-handle
// onMessage closure if any. The user wires this from TS via
// `webviewEvaluateJs(handle, "window.parent.postMessage(payload, '*')")`
// from inside the embedded page (or the embedded page can call
// `window.parent.postMessage(...)` directly). This is the standard
// cross-origin message channel — both sides must opt in.
function perry_ui_webview_create(url, _width, _height, _ephemeral) {
  const el = document.createElement("iframe");
  el.src = url || "about:blank";
  el.setAttribute("sandbox", "allow-scripts allow-same-origin allow-forms allow-popups");
  el.style.border = "0";
  el.style.width = "100%";
  el.style.height = "100%";
  el._perry_state = { allowed: [], onShould: null, onLoaded: null, onError: null };
  // v2-C: cross-origin postMessage is a TS-side concern — user code
  // calls `window.addEventListener("message", e => ...)` directly to
  // receive frames from the iframe's contentWindow. To send TO the
  // iframe, use `webviewEvaluateJs(handle, "window.postMessage(...)")`.
  // See the JSDoc on the WebView() type declaration for the working
  // pattern. We don't expose a cross-platform onMessage / postMessage
  // FFI in v1 because that's the Tauri/Electron path Perry's WebView
  // explicitly avoids per the #658 design discussion.
  el.addEventListener("load", () => {
    const st = el._perry_state;
    let url = "";
    try { url = el.contentWindow?.location?.href || el.src; } catch { url = el.src; }
    if (st.onShould && st.allowed.length > 0) {
      // Best-effort allowlist — for cross-origin URIs, contentWindow throws.
      try {
        const host = new URL(url).hostname;
        const ok = st.allowed.some(d => host === d || host.endsWith("." + d));
        if (!ok) { el.src = "about:blank"; return; }
      } catch {}
    }
    if (st.onLoaded) st.onLoaded(url);
  });
  el.addEventListener("error", () => {
    const st = el._perry_state;
    if (st.onError) st.onError(0, "iframe load error");
  });
  return uiAlloc(el);
}
function perry_ui_webview_set_user_agent(_h, _ua) { /* not configurable from JS */ }
function perry_ui_webview_set_allowed_domains(h, arr) {
  const el = uiGet(h); if (!el || !el._perry_state) return;
  el._perry_state.allowed = Array.isArray(arr) ? arr.slice() : [];
}
function perry_ui_webview_set_ephemeral(_h, _e) { /* iframe storage = parent storage; isolation needs a fresh tab */ }
function perry_ui_webview_set_on_should_navigate(h, cb) { const el = uiGet(h); if (el?._perry_state) el._perry_state.onShould = cb; }
function perry_ui_webview_set_on_loaded(h, cb) { const el = uiGet(h); if (el?._perry_state) el._perry_state.onLoaded = cb; }
function perry_ui_webview_set_on_error(h, cb) { const el = uiGet(h); if (el?._perry_state) el._perry_state.onError = cb; }
function perry_ui_webview_load_url(h, url) { const el = uiGet(h); if (el) el.src = url; }
function perry_ui_webview_reload(h) {
  const el = uiGet(h); if (!el) return;
  try { el.contentWindow?.location?.reload(); } catch { el.src = el.src; }
}
function perry_ui_webview_go_back(h) { const el = uiGet(h); if (!el) return; try { el.contentWindow?.history?.back(); } catch {} }
function perry_ui_webview_go_forward(h) { const el = uiGet(h); if (!el) return; try { el.contentWindow?.history?.forward(); } catch {} }
function perry_ui_webview_can_go_back(h) {
  const el = uiGet(h); if (!el) return 0;
  try { return (el.contentWindow?.history?.length || 0) > 1 ? 1 : 0; } catch { return 0; }
}
function perry_ui_webview_evaluate_js(h, js, cb) {
  const el = uiGet(h); if (!el) { if (cb) cb(""); return; }
  // Same-origin only — cross-origin contentWindow access throws.
  try {
    const result = el.contentWindow?.eval(js);
    if (cb) cb(result == null ? "" : String(result));
  } catch {
    if (cb) cb("");
  }
}
function perry_ui_webview_clear_cookies(_h) { /* iframe shares storage with parent */ }

// ---------- ProgressView ----------
function perry_ui_progressview_set_value(h, value) { const el = uiGet(h); if (el) { el.max = 1; el.value = value; } }

// ---------- Canvas ----------
function perry_ui_canvas_fill_rect(h, x, y, w, ht) { const el = uiGet(h); if (el?._ctx) el._ctx.fillRect(x, y, w, ht); }
function perry_ui_canvas_stroke_rect(h, x, y, w, ht) { const el = uiGet(h); if (el?._ctx) el._ctx.strokeRect(x, y, w, ht); }
function perry_ui_canvas_clear_rect(h, x, y, w, ht) { const el = uiGet(h); if (el?._ctx) el._ctx.clearRect(x, y, w || el.width, ht || el.height); }
function perry_ui_canvas_set_fill_color(h, r, g, b, a) { const el = uiGet(h); if (el?._ctx) el._ctx.fillStyle = `rgba(${r*255|0},${g*255|0},${b*255|0},${a})`; }
function perry_ui_canvas_set_stroke_color(h, r, g, b, a) { const el = uiGet(h); if (el?._ctx) el._ctx.strokeStyle = `rgba(${r*255|0},${g*255|0},${b*255|0},${a})`; }
function perry_ui_canvas_begin_path(h) { const el = uiGet(h); if (el?._ctx) el._ctx.beginPath(); }
function perry_ui_canvas_move_to(h, x, y) { const el = uiGet(h); if (el?._ctx) el._ctx.moveTo(x, y); }
function perry_ui_canvas_line_to(h, x, y) { const el = uiGet(h); if (el?._ctx) el._ctx.lineTo(x, y); }
function perry_ui_canvas_arc(h, x, y, radius, startAngle, endAngle) { const el = uiGet(h); if (el?._ctx) el._ctx.arc(x, y, radius, startAngle, endAngle); }
function perry_ui_canvas_close_path(h) { const el = uiGet(h); if (el?._ctx) el._ctx.closePath(); }
function perry_ui_canvas_fill(h) { const el = uiGet(h); if (el?._ctx) el._ctx.fill(); }
function perry_ui_canvas_stroke(h) { const el = uiGet(h); if (el?._ctx) el._ctx.stroke(); }
function perry_ui_canvas_set_line_width(h, w) { const el = uiGet(h); if (el?._ctx) el._ctx.lineWidth = w; }
function perry_ui_canvas_fill_text(h, text, x, y) { const el = uiGet(h); if (el?._ctx) el._ctx.fillText(text, x, y); }
function perry_ui_canvas_set_font(h, font) { const el = uiGet(h); if (el?._ctx) el._ctx.font = font; }

function perry_ui_load_image(url) {
    const key = String(url || "");
    if (perryImageCache.has(key)) return perryImageCache.get(key).promise;
    const img = new Image();
    const asset = { url: key, element: img, width: 0, height: 0, ready: false, error: null };
    const promise = new Promise((resolve, reject) => {
        img.onload = () => {
            asset.width = img.naturalWidth || img.width || 0;
            asset.height = img.naturalHeight || img.height || 0;
            asset.ready = true;
            resolve(asset);
        };
        img.onerror = () => {
            asset.error = new Error(`Failed to load image: ${key}`);
            reject(asset.error);
        };
        img.src = key;
        if (img.decode) img.decode().then(() => {
            if (!asset.ready) {
                asset.width = img.naturalWidth || img.width || 0;
                asset.height = img.naturalHeight || img.height || 0;
                asset.ready = true;
                resolve(asset);
            }
        }).catch(() => { /* onerror covers network/decode failures */ });
    });
    asset.promise = promise;
    perryImageCache.set(key, asset);
    return promise;
}

function perry_ui_canvas_draw_image(h, image, ...args) {
    const el = (typeof getHandle === "function") ? getHandle(h) : uiGet(h);
    const ctx = el && el._ctx;
    const asset = image && image.element ? image : null;
    if (!ctx || !asset || !asset.ready || !asset.element) return;
    if (args.length === 2) {
        ctx.drawImage(asset.element, args[0], args[1]);
    } else if (args.length === 4) {
        ctx.drawImage(asset.element, args[0], args[1], args[2], args[3]);
    } else if (args.length >= 8) {
        const [sx, sy, sw, sh, dx, dy, dw, dh] = args;
        const srcW = sw > 0 ? sw : asset.width;
        const srcH = sh > 0 ? sh : asset.height;
        const dstW = dw > 0 ? dw : srcW;
        const dstH = dh > 0 ? dh : srcH;
        if (srcW <= 0 || srcH <= 0 || dstW <= 0 || dstH <= 0) return;
        ctx.drawImage(asset.element, sx, sy, srcW, srcH, dx, dy, dstW, dstH);
    }
}

function perry_ui_canvas_fill_gradient(h, r1, g1, b1, a1, r2, g2, b2, a2, direction) {
  const el = uiGet(h); if (!el?._ctx) return;
  const ctx = el._ctx;
  const grad = direction === 1 ? ctx.createLinearGradient(0, 0, el.width, 0) : ctx.createLinearGradient(0, 0, 0, el.height);
  grad.addColorStop(0, `rgba(${r1*255|0},${g1*255|0},${b1*255|0},${a1})`);
  grad.addColorStop(1, `rgba(${r2*255|0},${g2*255|0},${b2*255|0},${a2})`);
  ctx.fillStyle = grad; ctx.fill();
}

// ---------- Menu ----------
function perry_ui_menu_create() { const m = document.createElement("div"); m._items = []; return uiAlloc(m); }
function perry_ui_menu_add_item(menuH, title, callback) {
  const m = uiGet(menuH); if (!m) return;
  m._items.push({ title, callback, shortcut: null });
}
function perry_ui_menu_add_item_with_shortcut(menuH, title, callback, shortcut) {
  const m = uiGet(menuH); if (!m) return;
  m._items.push({ title, callback, shortcut });
}
function perry_ui_menu_add_separator(menuH) { const m = uiGet(menuH); if (m) m._items.push({ separator: true }); }
function perry_ui_menu_add_submenu(menuH, title, submenuH) {
  const m = uiGet(menuH); if (m) m._items.push({ title, submenu: submenuH });
}
function perry_ui_menu_clear(menuH) { const m = uiGet(menuH); if (m) m._items.length = 0; }
function perry_ui_menu_add_standard_action(menuH, title, selector, shortcut) { perry_ui_menu_add_item_with_shortcut(menuH, title, undefined, shortcut); }
function perry_ui_menubar_create() { const bar = document.createElement("div"); bar.style.display = "flex"; bar.style.background = "#f0f0f0"; bar.style.padding = "2px 8px"; bar._menus = []; return uiAlloc(bar); }
function perry_ui_menubar_add_menu(barH, title, menuH) {
  const bar = uiGet(barH); if (!bar) return;
  bar._menus.push({ title, menuH });
  const btn = document.createElement("button");
  btn.textContent = title; btn.style.border = "none"; btn.style.background = "transparent"; btn.style.padding = "4px 8px"; btn.style.cursor = "pointer";
  bar.appendChild(btn);
}
function perry_ui_menubar_attach(barH) {
  const bar = uiGet(barH); if (!bar) return;
  const root = uiGetRoot(); root.insertBefore(bar, root.firstChild);
}

// ---------- Clipboard ----------
function perry_ui_clipboard_read() { /* async in browser, return empty for now */ return ""; }
function perry_ui_clipboard_write(text) { try { navigator.clipboard.writeText(text); } catch(e) {} }

// ---------- Dialog ----------
function perry_ui_open_file_dialog(callback) {
  const input = document.createElement("input"); input.type = "file";
  input.addEventListener("change", () => { if (input.files.length) callWasmClosure(callback, fromJsValue(input.files[0].name)); });
  input.click();
}
function perry_ui_open_folder_dialog(callback) { perry_ui_open_file_dialog(callback); }
function perry_ui_save_file_dialog(callback, defaultName) {
  const name = prompt("Save as:", defaultName || "file.txt");
  if (name) callWasmClosure(callback, fromJsValue(name));
}
function perry_ui_alert(title, message, buttons, callback) {
  alert((title || "") + "\n" + (message || ""));
  if (callback !== undefined) callWasmClosure(callback, 0);
}

// ---------- Keyboard ----------

// Continuous keyboard events (issue #1864). Mirrors the macOS contract:
// one document-level listener routes events to whichever widget owns
// `__perryFocusedWidget`, with a `0`-keyed fallback for app-level handlers.
//
// `KeyboardEvent.code` is normalised to the same canonical names declared in
// `perry-ui::keys` (lowercase letters, "ArrowLeft", "Space", …). Names not in
// the canonical set are ignored — same behaviour as the native LUT.
const __perryKeyDownCBs = new Map();
const __perryKeyUpCBs = new Map();
let __perryFocusedWidget = 0;
let __perryAppKeyDownCB; // f64 callback
let __perryAppKeyUpCB;
const __perryHeldKeys = new Set();
let __perryCurrentMods = 0;
let __perryKbdInstalled = false;

// `KeyboardEvent.code` → numeric `Key` enum value (matches perry-ui::keys).
// Returns 0 (`Key.Unknown`) for codes outside the canonical set.
function __perryCodeToKey(code) {
  if (code.length === 4 && code.startsWith("Key")) {
    const c = code.charCodeAt(3);
    if (c >= 65 && c <= 90) return c - 64; // KeyA -> 1
  }
  if (code.length === 6 && code.startsWith("Digit")) {
    const c = code.charCodeAt(5);
    if (c >= 48 && c <= 57) return c - 21; // Digit0 -> 27
  }
  if (code.length >= 2 && code.charAt(0) === "F") {
    const n = parseInt(code.slice(1), 10);
    if (n >= 1 && n <= 12) return 36 + n;  // F1 -> 37
    if (n >= 13 && n <= 20) return 62 + n; // F13 -> 75
  }
  if (code.length >= 6 && code.startsWith("Numpad")) {
    const tail = code.slice(6);
    if (tail.length === 1) {
      const c = tail.charCodeAt(0);
      if (c >= 48 && c <= 57) return 83 + (c - 48); // Numpad0 -> 83
    }
    switch (tail) {
      case "Decimal": return 93;
      case "Enter": return 94;
      case "Add": return 95;
      case "Subtract": return 96;
      case "Multiply": return 97;
      case "Divide": return 98;
      case "Equal": return 99;
      case "Clear": return 100;
    }
  }
  switch (code) {
    case "ArrowUp": return 49;
    case "ArrowDown": return 50;
    case "ArrowLeft": return 51;
    case "ArrowRight": return 52;
    case "Space": return 53;
    case "Enter": case "NumpadEnter": return 54;
    case "Tab": return 55;
    case "Escape": return 56;
    case "Backspace": return 57;
    case "Delete": return 58;
    case "Home": return 59;
    case "End": return 60;
    case "PageUp": return 61;
    case "PageDown": return 62;
    case "Insert": return 63;
    case "Minus": return 64;
    case "Equal": return 65;
    case "BracketLeft": return 66;
    case "BracketRight": return 67;
    case "Backslash": return 68;
    case "Semicolon": return 69;
    case "Quote": return 70;
    case "Comma": return 71;
    case "Period": return 72;
    case "Slash": return 73;
    case "Backquote": return 74;
    default: return 0;
  }
}
function __perryEventMods(e) {
  return (e.metaKey ? 1 : 0) | (e.shiftKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.ctrlKey ? 8 : 0);
}
function __perryEnsureKbdInstalled() {
  if (__perryKbdInstalled) return;
  __perryKbdInstalled = true;
  // `keydown`/`keyup` update both held set and current modifier snapshot.
  // The DOM gives us modifier state on every event including modifier-only
  // presses (Shift down alone fires keydown with `Shift` code), so we don't
  // need a separate `flagsChanged`-equivalent listener.
  document.addEventListener("keydown", (e) => {
    __perryCurrentMods = __perryEventMods(e);
    const key = __perryCodeToKey(e.code);
    if (!key) return;
    __perryHeldKeys.add(key);
    const cb = __perryKeyDownCBs.get(__perryFocusedWidget) ?? __perryAppKeyDownCB;
    if (cb !== undefined) {
      callWasmClosure(cb, key, __perryCurrentMods, e.repeat ? 1 : 0);
    }
  });
  document.addEventListener("keyup", (e) => {
    __perryCurrentMods = __perryEventMods(e);
    const key = __perryCodeToKey(e.code);
    if (!key) return;
    __perryHeldKeys.delete(key);
    const cb = __perryKeyUpCBs.get(__perryFocusedWidget) ?? __perryAppKeyUpCB;
    if (cb !== undefined) {
      callWasmClosure(cb, key, __perryCurrentMods);
    }
  });
}
function perry_ui_widget_set_on_key_down(handle, callback) {
  __perryEnsureKbdInstalled();
  __perryKeyDownCBs.set(Number(handle), callback);
}
function perry_ui_widget_set_on_key_up(handle, callback) {
  __perryEnsureKbdInstalled();
  __perryKeyUpCBs.set(Number(handle), callback);
}
function perry_ui_app_set_on_key_down(callback) {
  __perryEnsureKbdInstalled();
  __perryAppKeyDownCB = callback;
}
function perry_ui_app_set_on_key_up(callback) {
  __perryEnsureKbdInstalled();
  __perryAppKeyUpCB = callback;
}
function perry_ui_focus_widget(handle) { __perryFocusedWidget = Number(handle); }
function perry_ui_blur_widget(handle) {
  if (__perryFocusedWidget === Number(handle)) __perryFocusedWidget = 0;
}
function perry_ui_is_key_down(keyCode) {
  __perryEnsureKbdInstalled();
  return __perryHeldKeys.has(Number(keyCode)) ? 1 : 0;
}
function perry_ui_current_modifiers() {
  __perryEnsureKbdInstalled();
  return __perryCurrentMods;
}

function perry_ui_register_global_hotkey(_key, _modifiers, _callback) {
  // Global hotkeys require OS-level hook APIs not available in a browser context.
}
function perry_ui_add_keyboard_shortcut(key, modifiers, callback) {
  document.addEventListener("keydown", (e) => {
    const mods = modifiers || 0;
    if ((mods & 1) && !e.metaKey) return; // Cmd
    if ((mods & 2) && !e.shiftKey) return;
    if ((mods & 4) && !e.altKey) return;
    if ((mods & 8) && !e.ctrlKey) return;
    if (e.key.toLowerCase() === (key || "").toLowerCase()) { e.preventDefault(); callWasmClosure(callback); }
  });
}

// ---------- Sheet ----------
function perry_ui_sheet_create(width, height, title) {
  const overlay = document.createElement("div");
  overlay.style.cssText = "position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.3);display:none;justify-content:center;align-items:center;z-index:1000";
  const panel = document.createElement("div");
  panel.style.cssText = `background:white;border-radius:12px;padding:16px;width:${width||400}px;max-height:${height||300}px;overflow:auto`;
  if (title) { const t = document.createElement("h3"); t.textContent = title; panel.appendChild(t); }
  overlay.appendChild(panel); document.body.appendChild(overlay);
  overlay._panel = panel;
  return uiAlloc(overlay);
}
function perry_ui_sheet_present(sheetH) { const el = uiGet(sheetH); if (el) el.style.display = "flex"; }
function perry_ui_sheet_dismiss(sheetH) { const el = uiGet(sheetH); if (el) el.style.display = "none"; }

// ---------- Toolbar ----------
function perry_ui_toolbar_create() {
  const bar = document.createElement("div");
  bar.style.cssText = "display:flex;gap:8px;padding:4px 8px;background:#f5f5f5;border-bottom:1px solid #ddd";
  return uiAlloc(bar);
}
function perry_ui_toolbar_add_item(toolbarH, label, icon, callback) {
  const bar = uiGet(toolbarH); if (!bar) return;
  const btn = document.createElement("button"); btn.textContent = label || icon || "";
  btn.style.cssText = "border:none;background:transparent;padding:4px 8px;cursor:pointer";
  btn.addEventListener("click", () => callWasmClosure(callback));
  bar.appendChild(btn);
}
function perry_ui_toolbar_attach(toolbarH) {
  const bar = uiGet(toolbarH); if (!bar) return;
  const root = uiGetRoot(); root.insertBefore(bar, root.firstChild);
}

// ---------- Window ----------
function perry_ui_window_create(title, width, height) {
  const win = document.createElement("div");
  win.style.cssText = `position:fixed;top:50px;left:50px;width:${width||400}px;height:${height||300}px;background:white;border:1px solid #ccc;border-radius:8px;box-shadow:0 4px 16px rgba(0,0,0,0.2);overflow:hidden;display:flex;flex-direction:column;z-index:999`;
  const titleBar = document.createElement("div");
  titleBar.style.cssText = "padding:8px 12px;background:#f0f0f0;border-bottom:1px solid #ddd;cursor:move;font-weight:600;font-size:13px";
  titleBar.textContent = title || "";
  win.appendChild(titleBar);
  const body = document.createElement("div"); body.style.cssText = "flex:1 1 0%;overflow:auto;padding:8px";
  win.appendChild(body); win._body = body;
  document.body.appendChild(win);
  return uiAlloc(win);
}
function perry_ui_window_set_body(windowH, widgetH) {
  const win = uiGet(windowH), w = uiGet(widgetH);
  if (win && w && win._body) { win._body.innerHTML = ""; win._body.appendChild(w); }
}
function perry_ui_window_show(windowH) { const win = uiGet(windowH); if (win) win.style.display = "flex"; }
function perry_ui_window_close(windowH) { const win = uiGet(windowH); if (win) win.style.display = "none"; }

// ---------- App lifecycle ----------
function perry_ui_app_run() { /* In browser, app is already running */ }
function perry_ui_app_set_body(appH, rootH) {
  const app = uiGet(appH), root = uiGet(rootH);
  if (app && root) { app.innerHTML = ""; app.appendChild(root); }
}
function perry_ui_app_set_min_size(appH, w, h) { /* no-op in web */ }
function perry_ui_app_set_max_size(appH, w, h) { /* no-op in web */ }
function perry_ui_app_on_activate(callback) { document.addEventListener("visibilitychange", () => { if (!document.hidden) callWasmClosure(callback); }); }
function perry_ui_app_on_terminate(callback) { window.addEventListener("beforeunload", () => callWasmClosure(callback)); }
function perry_ui_app_set_timer(intervalMs, callback) { setInterval(() => callWasmClosure(callback), intervalMs); }

// ---------- System APIs ----------
function perry_system_open_url(url) { window.open(url, "_blank"); }
function perry_system_is_dark_mode() { return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? 1 : 0; }
function perry_system_preferences_get(key) { return localStorage.getItem(key) || ""; }
function perry_system_preferences_set(key, value) { localStorage.setItem(key, value); }
// perry/system hapticPlay — Vibration API (no-op where unsupported, e.g. Safari).
function perry_system_haptic_play(type) {
  if (typeof navigator === "undefined" || typeof navigator.vibrate !== "function") return;
  const patterns = {
    success: 50, error: [50, 80, 50], warning: [30, 50, 30],
    light: 10, medium: 20, heavy: 40, click: 10, selection: 10,
    directionUp: 15, directionDown: 15, start: 25, stop: 25,
  };
  try { navigator.vibrate(patterns[type] ?? 10); } catch (e) { /* no-op */ }
}
function perry_system_keychain_save(key, value) { try { localStorage.setItem("__pk_" + key, value); } catch(e) {} }
function perry_system_keychain_get(key) { return localStorage.getItem("__pk_" + key) || ""; }
function perry_system_keychain_delete(key) { localStorage.removeItem("__pk_" + key); }
function perry_system_notification_send(title, body) {
  if ("Notification" in window && Notification.permission === "granted") new Notification(title, { body });
  else if ("Notification" in window) Notification.requestPermission().then(p => { if (p === "granted") new Notification(title, { body }); });
}

// Frame split (stub)
function perry_ui_frame_split_create() { return perry_ui_hstack_create(0); }
function perry_ui_frame_split_add_child(splitH, childH) { perry_ui_widget_add_child(splitH, childH); }

// ---------- Camera (issue #191) ----------
// Browser stubs. The Web target has no `getUserMedia`-backed live preview
// integrated yet, so these return sentinel values matching the documented
// contract: `cameraSampleColor` returns `-1` when no frame is available, the
// other setters are no-ops. User code calling `CameraView()` from a browser
// build resolves the symbol cleanly; on iOS/Android the same source compiles
// to the real AVCaptureSession / Camera2 backend.
function perry_ui_camera_create() {
  // Render a placeholder div so layout slots reserve space the same way
  // they would for a real preview. No camera permission is requested.
  const el = document.createElement("div");
  el.style.background = "#000";
  el.style.color = "#888";
  el.style.display = "flex";
  el.style.alignItems = "center";
  el.style.justifyContent = "center";
  el.style.minWidth = "200px";
  el.style.minHeight = "200px";
  el.textContent = "[camera preview not supported on web]";
  return uiAlloc(el);
}
function perry_ui_camera_start(_h) {}
function perry_ui_camera_stop(_h) {}
function perry_ui_camera_freeze(_h) {}
function perry_ui_camera_unfreeze(_h) {}
function perry_ui_camera_sample_color(_x, _y) { return -1; }
function perry_ui_camera_set_on_tap(_h, _cb) {}

// ---------- perry/media (issue #370) — HTML5 <audio> + Media Session API ----------
//
// Per-process player table. Index 0 is reserved (handles are 1-based).
// Each entry: { audio, state, duration, hasStarted, ended, onStateChange?, onTimeUpdate? }.
const PERRY_MEDIA_PLAYERS = [];
let PERRY_MEDIA_SESSION_WIRED = false;

function _perry_media_get(handle) {
  // handle arrives as a JS number (NaN-boxed values are decoded by mem_call before dispatch).
  if (typeof handle !== "number" || handle < 1) return null;
  return PERRY_MEDIA_PLAYERS[handle - 1] || null;
}

function _perry_media_flush_state(handle) {
  const e = _perry_media_get(handle);
  if (!e || e.onStateChange === undefined || e.onStateChange === null) return;
  try {
    callWasmClosure(e.onStateChange, fromJsValue(e.state));
  } catch (err) { console.warn("perry/media onStateChange threw:", err); }
}

function _perry_media_flush_time(handle) {
  const e = _perry_media_get(handle);
  if (!e || e.onTimeUpdate === undefined || e.onTimeUpdate === null) return;
  const t = isFinite(e.audio.currentTime) ? e.audio.currentTime : 0;
  const d = isFinite(e.duration) ? e.duration : 0;
  try {
    callWasmClosure(e.onTimeUpdate, t, d);
  } catch (err) { console.warn("perry/media onTimeUpdate threw:", err); }
}

function _perry_media_first_live_handle() {
  for (let i = 0; i < PERRY_MEDIA_PLAYERS.length; i++) {
    if (PERRY_MEDIA_PLAYERS[i]) return i + 1;
  }
  return 0;
}

function _perry_media_wire_session() {
  if (PERRY_MEDIA_SESSION_WIRED) return;
  if (!('mediaSession' in navigator)) return;
  PERRY_MEDIA_SESSION_WIRED = true;
  try {
    navigator.mediaSession.setActionHandler('play', () => {
      const h = _perry_media_first_live_handle();
      if (h) perry_media_play(h);
    });
    navigator.mediaSession.setActionHandler('pause', () => {
      const h = _perry_media_first_live_handle();
      if (h) perry_media_pause(h);
    });
    navigator.mediaSession.setActionHandler('seekto', (details) => {
      const h = _perry_media_first_live_handle();
      if (h && typeof details.seekTime === "number") perry_media_seek(h, details.seekTime);
    });
    navigator.mediaSession.setActionHandler('seekforward', (details) => {
      const h = _perry_media_first_live_handle();
      if (!h) return;
      const e = _perry_media_get(h);
      if (!e) return;
      const skip = (details && details.seekOffset) || 10;
      perry_media_seek(h, (e.audio.currentTime || 0) + skip);
    });
    navigator.mediaSession.setActionHandler('seekbackward', (details) => {
      const h = _perry_media_first_live_handle();
      if (!h) return;
      const e = _perry_media_get(h);
      if (!e) return;
      const skip = (details && details.seekOffset) || 10;
      perry_media_seek(h, Math.max(0, (e.audio.currentTime || 0) - skip));
    });
  } catch (err) { /* setActionHandler can throw on unsupported actions — ignore */ }
}

function perry_media_create_player(url) {
  const audio = new Audio(typeof url === "string" ? url : "");
  audio.preload = "auto";
  const entry = { audio, state: "loading", duration: 0, hasStarted: false, ended: false };
  PERRY_MEDIA_PLAYERS.push(entry);
  const handle = PERRY_MEDIA_PLAYERS.length; // 1-based
  audio.addEventListener('loadedmetadata', () => {
    entry.duration = isFinite(audio.duration) ? audio.duration : 0;
  });
  audio.addEventListener('canplay', () => {
    if (entry.state === 'loading') entry.state = 'ready';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('play', () => {
    entry.state = 'playing';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('playing', () => {
    entry.state = 'playing';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('pause', () => {
    if (entry.ended) return;
    entry.state = entry.hasStarted ? 'paused' : 'ready';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('ended', () => {
    entry.ended = true;
    entry.state = 'ended';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('error', () => {
    entry.state = 'error';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('waiting', () => {
    entry.state = 'loading';
    _perry_media_flush_state(handle);
  });
  audio.addEventListener('timeupdate', () => {
    // Belt-and-braces ended fallback (acroyear #351 comment) — Chromium /
    // Chromecast have historically dropped the `ended` event.
    if (entry.hasStarted && entry.duration > 0.25 && audio.currentTime >= entry.duration - 0.25 && !entry.ended) {
      entry.ended = true;
      entry.state = 'ended';
      _perry_media_flush_state(handle);
    }
    if (entry.state === 'playing' || entry.state === 'loading') {
      _perry_media_flush_time(handle);
    }
  });
  return handle;
}

function perry_media_play(handle) {
  const e = _perry_media_get(handle);
  if (!e) return;
  e.hasStarted = true;
  e.ended = false;
  const p = e.audio.play();
  if (p && typeof p.catch === "function") {
    p.catch(() => { e.state = 'error'; _perry_media_flush_state(handle); });
  }
}

function perry_media_pause(handle) {
  const e = _perry_media_get(handle);
  if (!e) return;
  e.audio.pause();
}

function perry_media_stop(handle) {
  const e = _perry_media_get(handle);
  if (!e) return;
  e.audio.pause();
  try { e.audio.currentTime = 0; } catch (_) { /* live stream */ }
  e.hasStarted = false;
  e.ended = false;
  e.state = 'ready';
  _perry_media_flush_state(handle);
}

function perry_media_seek(handle, seconds) {
  const e = _perry_media_get(handle);
  if (!e) return;
  const d = e.audio.duration;
  let target = Number(seconds) || 0;
  if (target < 0) target = 0;
  if (isFinite(d) && target > d) target = d;
  try { e.audio.currentTime = target; } catch (_) { /* live stream */ }
}

function perry_media_set_volume(handle, vol) {
  const e = _perry_media_get(handle);
  if (!e) return;
  let v = Number(vol);
  if (!isFinite(v)) v = 1;
  if (v < 0) v = 0;
  if (v > 1) v = 1;
  e.audio.volume = v;
}

function perry_media_set_rate(handle, rate) {
  const e = _perry_media_get(handle);
  if (!e) return;
  const r = Number(rate);
  if (!isFinite(r) || r <= 0) return;
  e.audio.playbackRate = r;
}

function perry_media_get_current_time(handle) {
  const e = _perry_media_get(handle);
  if (!e) return 0;
  return isFinite(e.audio.currentTime) ? e.audio.currentTime : 0;
}

function perry_media_get_duration(handle) {
  const e = _perry_media_get(handle);
  if (!e) return 0;
  return isFinite(e.duration) ? e.duration : 0;
}

function perry_media_get_state(handle) {
  const e = _perry_media_get(handle);
  return e ? e.state : "error";
}

function perry_media_is_playing(handle) {
  const e = _perry_media_get(handle);
  return (e && e.state === 'playing') ? 1 : 0;
}

function perry_media_on_state_change(handle, callback) {
  const e = _perry_media_get(handle);
  if (!e) return;
  // callback comes through mem_call as a decoded JS value — either a JS
  // function (closure) or a wasm-closure handle object {funcIdx, captures}.
  // Both shapes are handled by callWasmClosure.
  e.onStateChange = (callback === undefined || callback === null) ? null : callback;
}

function perry_media_on_time_update(handle, callback) {
  const e = _perry_media_get(handle);
  if (!e) return;
  e.onTimeUpdate = (callback === undefined || callback === null) ? null : callback;
}

function perry_media_set_now_playing(handle, title, artist, album, artworkUrl) {
  const e = _perry_media_get(handle);
  if (!e) return;
  if (!('mediaSession' in navigator)) return;
  try {
    const meta = {
      title: String(title || ""),
      artist: String(artist || ""),
      album: String(album || ""),
    };
    if (artworkUrl && typeof MediaMetadata !== "undefined") {
      meta.artwork = [{ src: String(artworkUrl) }];
    }
    navigator.mediaSession.metadata = (typeof MediaMetadata !== "undefined")
      ? new MediaMetadata(meta)
      : meta;
    _perry_media_wire_session();
  } catch (err) { console.warn("perry/media setNowPlaying:", err); }
}

function perry_media_destroy(handle) {
  const e = _perry_media_get(handle);
  if (!e) return;
  try { e.audio.pause(); } catch (_) {}
  try { e.audio.src = ""; } catch (_) {}
  e.onStateChange = null;
  e.onTimeUpdate = null;
  PERRY_MEDIA_PLAYERS[handle - 1] = null;
}

// ---------- perry/audio (issue #1867) — Web Audio API: low-latency SFX + buses ----------
//
// Distinct from perry/media (streaming + lock-screen). This is the game-audio path:
// preloaded PCM, voice pool, bus hierarchy, pitch/pan/fade/loop.
//
// Handle ranges (must match Apple impl):
//   Sound:      0x00000001..0x0FFFFFFF
//   PlaybackId: 0x10000001..0x1FFFFFFF
//   Bus:        0x20000001..0x2FFFFFFF
//   0 = master bus
const PERRY_AUDIO_SOUND_BASE = 0x00000000;
const PERRY_AUDIO_PLAYBACK_BASE = 0x10000000;
const PERRY_AUDIO_BUS_BASE = 0x20000000;
const PERRY_AUDIO_RANGE_MASK = 0xF0000000;

const PERRY_AUDIO_SOUNDS = [];     // index = (handle - 1)
const PERRY_AUDIO_PLAYBACKS = [];  // index = (handle - PERRY_AUDIO_PLAYBACK_BASE - 1)
const PERRY_AUDIO_BUSES = [];      // index 0 = master (handle 0 maps here too)

let PERRY_AUDIO_CTX = null;
let PERRY_AUDIO_MASTER_GAIN = null;
let PERRY_AUDIO_MASTER_BUS = null;
let PERRY_AUDIO_WARNED_AUTOPLAY = false;

function _perry_audio_ctx() {
  if (PERRY_AUDIO_CTX) return PERRY_AUDIO_CTX;
  try {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) {
      console.warn("perry/audio: Web Audio API not available");
      return null;
    }
    PERRY_AUDIO_CTX = new AC();
    PERRY_AUDIO_MASTER_GAIN = PERRY_AUDIO_CTX.createGain();
    PERRY_AUDIO_MASTER_GAIN.gain.value = 1.0;
    PERRY_AUDIO_MASTER_GAIN.connect(PERRY_AUDIO_CTX.destination);
    // Master bus = index 0; handle 0 dereferences here.
    PERRY_AUDIO_MASTER_BUS = {
      name: "master",
      parent_id: -1,
      gainNode: PERRY_AUDIO_MASTER_GAIN,
      stored_volume: 1.0,
      muted: false,
      soloed: false,
      destroyed: false,
    };
    PERRY_AUDIO_BUSES[0] = PERRY_AUDIO_MASTER_BUS;
  } catch (err) {
    console.warn("perry/audio: failed to create AudioContext:", err);
  }
  return PERRY_AUDIO_CTX;
}

function _perry_audio_handle_kind(h) {
  const n = Number(h) >>> 0;
  if (n === 0) return "master";
  const range = n & PERRY_AUDIO_RANGE_MASK;
  if (range === PERRY_AUDIO_BUS_BASE) return "bus";
  if (range === PERRY_AUDIO_PLAYBACK_BASE) return "playback";
  return "sound";
}

function _perry_audio_get_sound(h) {
  const n = Number(h) >>> 0;
  if (n === 0 || (n & PERRY_AUDIO_RANGE_MASK) !== 0) return null;
  return PERRY_AUDIO_SOUNDS[n - 1] || null;
}
function _perry_audio_get_playback(h) {
  const n = Number(h) >>> 0;
  if ((n & PERRY_AUDIO_RANGE_MASK) !== PERRY_AUDIO_PLAYBACK_BASE) return null;
  return PERRY_AUDIO_PLAYBACKS[n - PERRY_AUDIO_PLAYBACK_BASE - 1] || null;
}
function _perry_audio_get_bus(h) {
  const n = Number(h) >>> 0;
  if (n === 0) return PERRY_AUDIO_MASTER_BUS;
  if ((n & PERRY_AUDIO_RANGE_MASK) !== PERRY_AUDIO_BUS_BASE) return null;
  return PERRY_AUDIO_BUSES[n - PERRY_AUDIO_BUS_BASE] || null;
}

function _perry_audio_resolve_url(path) {
  try {
    return new URL(String(path), document.baseURI).href;
  } catch (_) {
    return String(path);
  }
}

function _perry_audio_set_gain(gainParam, target, fadeMs) {
  if (!PERRY_AUDIO_CTX) return;
  const t = Math.max(0, Number(target) || 0);
  const ms = Number(fadeMs) || 0;
  const now = PERRY_AUDIO_CTX.currentTime;
  try {
    gainParam.cancelScheduledValues(now);
    if (ms > 0) {
      const cur = gainParam.value;
      gainParam.setValueAtTime(cur, now);
      gainParam.linearRampToValueAtTime(t, now + ms / 1000);
    } else {
      gainParam.setValueAtTime(t, now);
    }
  } catch (err) {
    try { gainParam.value = t; } catch (_) {}
  }
}

function _perry_audio_start_voice(entry) {
  // Materialize an AudioBufferSourceNode for this playback entry.
  if (!PERRY_AUDIO_CTX || entry.stopped) return;
  const sound = entry.sound;
  if (!sound || !sound.audioBuffer) return;
  const bus = entry.bus || PERRY_AUDIO_MASTER_BUS;
  const src = PERRY_AUDIO_CTX.createBufferSource();
  src.buffer = sound.audioBuffer;
  src.loop = !!entry.loop;
  try { src.playbackRate.value = entry.rate; } catch (_) {}

  const voiceGain = PERRY_AUDIO_CTX.createGain();
  let tail = voiceGain;
  if (Math.abs(entry.pan) > 1e-6 && typeof PERRY_AUDIO_CTX.createStereoPanner === "function") {
    const panner = PERRY_AUDIO_CTX.createStereoPanner();
    try { panner.pan.value = Math.max(-1, Math.min(1, entry.pan)); } catch (_) {}
    voiceGain.connect(panner);
    tail = panner;
    entry.pannerNode = panner;
  }
  tail.connect(bus.gainNode);
  src.connect(voiceGain);

  const now = PERRY_AUDIO_CTX.currentTime;
  if (entry.fadeInMs > 0) {
    voiceGain.gain.setValueAtTime(0, now);
    voiceGain.gain.linearRampToValueAtTime(entry.volume, now + entry.fadeInMs / 1000);
  } else {
    voiceGain.gain.setValueAtTime(entry.volume, now);
  }

  entry.source = src;
  entry.gain = voiceGain;
  entry.startedAt = now - (entry.pauseOffset || 0);
  entry.pauseOffset = entry.pauseOffset || 0;

  src.onended = () => {
    if (entry.suppressEnded) { entry.suppressEnded = false; return; }
    if (entry.stopped) return;
    entry.stopped = true;
    entry.ended = true;
    if (entry.onEnded) {
      try { callWasmClosure(entry.onEnded); }
      catch (err) { console.warn("perry/audio onEnded threw:", err); }
    }
  };

  try { src.start(0, entry.pauseOffset || 0); }
  catch (err) { console.warn("perry/audio start failed:", err); }
}

function _perry_audio_apply_solo_state() {
  // When any bus is soloed, mute all non-soloed non-ancestor non-master buses.
  if (!PERRY_AUDIO_CTX) return;
  const soloed = PERRY_AUDIO_BUSES.filter(b => b && !b.destroyed && b.soloed);
  for (let i = 1; i < PERRY_AUDIO_BUSES.length; i++) {
    const b = PERRY_AUDIO_BUSES[i];
    if (!b || b.destroyed) continue;
    let audible;
    if (soloed.length === 0) audible = !b.muted;
    else audible = b.soloed && !b.muted;
    _perry_audio_set_gain(b.gainNode.gain, audible ? b.stored_volume : 0, 0);
  }
}

function perry_audio_load_sound(path, busHandle, stream) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return 0;
  const url = _perry_audio_resolve_url(path);
  const bus = _perry_audio_get_bus(busHandle) || PERRY_AUDIO_MASTER_BUS;
  const entry = {
    url,
    audioBuffer: null,
    default_volume: 1.0,
    default_bus: bus,
    pendingPlays: [],
    onLoaded: null,
    loaded: false,
    error: false,
    stream: !!Number(stream),
  };
  PERRY_AUDIO_SOUNDS.push(entry);
  const handle = PERRY_AUDIO_SOUNDS.length; // 1-based, within sound range
  fetch(url).then(r => {
    if (!r.ok) throw new Error("HTTP " + r.status);
    return r.arrayBuffer();
  }).then(ab => new Promise((resolve, reject) => {
    // decodeAudioData is older-API: pass callbacks for Safari compat.
    try {
      const p = ctx.decodeAudioData(ab, resolve, reject);
      if (p && typeof p.then === "function") p.then(resolve, reject);
    } catch (err) { reject(err); }
  })).then(buf => {
    entry.audioBuffer = buf;
    entry.loaded = true;
    // Flush queued plays.
    const queued = entry.pendingPlays;
    entry.pendingPlays = [];
    for (const voice of queued) {
      if (!voice.stopped) _perry_audio_start_voice(voice);
    }
    if (entry.onLoaded) {
      try { callWasmClosure(entry.onLoaded); }
      catch (err) { console.warn("perry/audio onLoaded threw:", err); }
    }
  }).catch(err => {
    entry.error = true;
    console.warn("perry/audio load failed for", url, err);
  });
  return handle;
}

function perry_audio_unload(soundHandle) {
  const s = _perry_audio_get_sound(soundHandle);
  if (!s) return;
  // Stop any live voices on this sound.
  for (let i = 0; i < PERRY_AUDIO_PLAYBACKS.length; i++) {
    const v = PERRY_AUDIO_PLAYBACKS[i];
    if (v && v.sound === s && !v.stopped) {
      try { v.suppressEnded = true; if (v.source) v.source.stop(); } catch (_) {}
      v.stopped = true;
    }
  }
  s.audioBuffer = null;
  s.pendingPlays = [];
  s.onLoaded = null;
  PERRY_AUDIO_SOUNDS[Number(soundHandle) - 1] = null;
}

function perry_audio_play(soundHandle, volume, loop_, rate, pan, fadeInMs) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return 0;
  const sound = _perry_audio_get_sound(soundHandle);
  if (!sound) return 0;
  // Autoplay-policy: if context is suspended (first play before user gesture)
  // attempt resume; the browser will queue audio until the gesture arrives.
  if (ctx.state === "suspended") {
    const p = ctx.resume();
    if (p && typeof p.catch === "function") p.catch(() => {
      if (!PERRY_AUDIO_WARNED_AUTOPLAY) {
        PERRY_AUDIO_WARNED_AUTOPLAY = true;
        console.warn("perry/audio: AudioContext is suspended — first play must follow a user gesture (click/keydown).");
      }
    });
  }
  const vol = (Number(volume) >= 0) ? Number(volume) : sound.default_volume;
  const entry = {
    sound,
    bus: sound.default_bus,
    volume: vol,
    loop: Number(loop_) >= 0.5,
    rate: (Number(rate) > 0) ? Number(rate) : 1.0,
    pan: Number(pan) || 0,
    fadeInMs: Math.max(0, Number(fadeInMs) || 0),
    source: null,
    gain: null,
    pannerNode: null,
    onEnded: null,
    stopped: false,
    ended: false,
    paused: false,
    pauseOffset: 0,
    startedAt: 0,
    suppressEnded: false,
  };
  PERRY_AUDIO_PLAYBACKS.push(entry);
  const handle = PERRY_AUDIO_PLAYBACK_BASE + PERRY_AUDIO_PLAYBACKS.length; // 1-based within playback range
  if (sound.loaded && sound.audioBuffer) {
    _perry_audio_start_voice(entry);
  } else if (!sound.error) {
    sound.pendingPlays.push(entry);
  } else {
    entry.stopped = true;
  }
  return handle;
}

function perry_audio_stop(handle, fadeOutMs) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return;
  const ms = Math.max(0, Number(fadeOutMs) || 0);
  const stopOne = (v) => {
    if (!v || v.stopped) return;
    if (ms > 0 && v.gain) {
      const now = ctx.currentTime;
      try {
        v.gain.gain.cancelScheduledValues(now);
        v.gain.gain.setValueAtTime(v.gain.gain.value, now);
        v.gain.gain.linearRampToValueAtTime(0, now + ms / 1000);
      } catch (_) {}
      v.stopped = true;
      setTimeout(() => {
        try { if (v.source) v.source.stop(); } catch (_) {}
      }, ms + 5);
    } else {
      v.stopped = true;
      try { if (v.source) v.source.stop(); } catch (_) {}
    }
  };
  const kind = _perry_audio_handle_kind(handle);
  if (kind === "playback") {
    stopOne(_perry_audio_get_playback(handle));
  } else if (kind === "sound") {
    const s = _perry_audio_get_sound(handle);
    if (s) {
      for (const v of PERRY_AUDIO_PLAYBACKS) if (v && v.sound === s) stopOne(v);
    }
  }
}

function perry_audio_pause(playbackHandle) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return;
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v || v.stopped || v.paused) return;
  // Web Audio source nodes can't pause — capture offset, stop, replay on resume.
  const elapsed = ctx.currentTime - v.startedAt;
  let offset = elapsed;
  if (v.sound && v.sound.audioBuffer && !v.loop) {
    offset = Math.min(elapsed, v.sound.audioBuffer.duration);
  } else if (v.sound && v.sound.audioBuffer && v.loop) {
    offset = elapsed % v.sound.audioBuffer.duration;
  }
  v.pauseOffset = offset;
  v.paused = true;
  v.suppressEnded = true;
  try { if (v.source) v.source.stop(); } catch (_) {}
  v.source = null;
}

function perry_audio_resume(playbackHandle) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return;
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v || v.stopped || !v.paused) return;
  v.paused = false;
  // Don't re-apply fade-in on resume.
  const saved = v.fadeInMs;
  v.fadeInMs = 0;
  _perry_audio_start_voice(v);
  v.fadeInMs = saved;
}

function perry_audio_set_volume(handle, volume, fadeMs) {
  if (!PERRY_AUDIO_CTX) return;
  const v = Math.max(0, Number(volume) || 0);
  const kind = _perry_audio_handle_kind(handle);
  if (kind === "playback") {
    const p = _perry_audio_get_playback(handle);
    if (p && p.gain) {
      p.volume = v;
      _perry_audio_set_gain(p.gain.gain, v, fadeMs);
    } else if (p) {
      p.volume = v;
    }
  } else if (kind === "bus" || kind === "master") {
    const b = _perry_audio_get_bus(handle);
    if (b) {
      b.stored_volume = v;
      if (!b.muted) _perry_audio_set_gain(b.gainNode.gain, v, fadeMs);
    }
  } else if (kind === "sound") {
    const s = _perry_audio_get_sound(handle);
    if (s) s.default_volume = v;
  }
}

function perry_audio_set_rate(playbackHandle, rate) {
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v) return;
  const r = Number(rate);
  if (!isFinite(r) || r <= 0) return;
  v.rate = r;
  if (v.source) {
    try { v.source.playbackRate.value = r; } catch (_) {}
  }
}

function perry_audio_set_pan(playbackHandle, pan) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return;
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v) return;
  const p = Math.max(-1, Math.min(1, Number(pan) || 0));
  v.pan = p;
  if (v.pannerNode) {
    try { v.pannerNode.pan.value = p; } catch (_) {}
  } else if (v.source && v.gain && typeof ctx.createStereoPanner === "function") {
    // Insert a panner mid-flight: voiceGain → panner → bus.
    try {
      const panner = ctx.createStereoPanner();
      panner.pan.value = p;
      v.gain.disconnect();
      v.gain.connect(panner);
      panner.connect((v.bus || PERRY_AUDIO_MASTER_BUS).gainNode);
      v.pannerNode = panner;
    } catch (err) { console.warn("perry/audio setPan:", err); }
  }
}

function perry_audio_fade_in(playbackHandle, durationMs, toVol) {
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v || !v.gain) return;
  const t = Number(toVol);
  const target = (isFinite(t) && t >= 0) ? t : v.volume;
  v.volume = target;
  _perry_audio_set_gain(v.gain.gain, target, durationMs);
}

function perry_audio_fade_out(playbackHandle, durationMs) {
  perry_audio_stop(playbackHandle, durationMs);
}

function perry_audio_crossfade(fromHandle, toHandle, durationMs) {
  const ms = Math.max(0, Number(durationMs) || 0);
  const from = _perry_audio_get_playback(fromHandle);
  const to = _perry_audio_get_playback(toHandle);
  if (from && from.gain) _perry_audio_set_gain(from.gain.gain, 0, ms);
  if (to && to.gain) _perry_audio_set_gain(to.gain.gain, to.volume || 1.0, ms);
  if (from) {
    from.stopped = true;
    const ctx = _perry_audio_ctx();
    setTimeout(() => { try { if (from.source) from.source.stop(); } catch (_) {} }, ms + 5);
  }
}

function perry_audio_create_bus(name, parentBus) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return 0;
  const parent = _perry_audio_get_bus(parentBus) || PERRY_AUDIO_MASTER_BUS;
  const gainNode = ctx.createGain();
  gainNode.gain.value = 1.0;
  gainNode.connect(parent.gainNode);
  const entry = {
    name: String(name || ""),
    parent_id: parent === PERRY_AUDIO_MASTER_BUS ? 0 : PERRY_AUDIO_BUSES.indexOf(parent),
    gainNode,
    stored_volume: 1.0,
    muted: false,
    soloed: false,
    destroyed: false,
  };
  // Index 0 reserved for master; user buses start at index 1.
  if (PERRY_AUDIO_BUSES.length === 0) PERRY_AUDIO_BUSES[0] = PERRY_AUDIO_MASTER_BUS;
  PERRY_AUDIO_BUSES.push(entry);
  const idx = PERRY_AUDIO_BUSES.length - 1;
  return PERRY_AUDIO_BUS_BASE + idx;
}

function perry_audio_destroy_bus(busHandle) {
  const b = _perry_audio_get_bus(busHandle);
  if (!b || b === PERRY_AUDIO_MASTER_BUS) return;
  try { b.gainNode.disconnect(); } catch (_) {}
  b.destroyed = true;
}

function perry_audio_mute_bus(busHandle, muted) {
  const b = _perry_audio_get_bus(busHandle);
  if (!b) return;
  const m = Number(muted) >= 0.5;
  b.muted = m;
  _perry_audio_apply_solo_state();
  if (!m && !PERRY_AUDIO_BUSES.some(x => x && x.soloed)) {
    _perry_audio_set_gain(b.gainNode.gain, b.stored_volume, 0);
  } else if (m) {
    _perry_audio_set_gain(b.gainNode.gain, 0, 0);
  }
}

function perry_audio_solo_bus(busHandle, soloed) {
  const b = _perry_audio_get_bus(busHandle);
  if (!b || b === PERRY_AUDIO_MASTER_BUS) return;
  b.soloed = Number(soloed) >= 0.5;
  _perry_audio_apply_solo_state();
}

function perry_audio_set_master_volume(volume, fadeMs) {
  if (!_perry_audio_ctx()) return;
  const v = Math.max(0, Number(volume) || 0);
  PERRY_AUDIO_MASTER_BUS.stored_volume = v;
  _perry_audio_set_gain(PERRY_AUDIO_MASTER_GAIN.gain, v, fadeMs);
}

function perry_audio_suspend() {
  if (!PERRY_AUDIO_CTX) return;
  try { PERRY_AUDIO_CTX.suspend(); } catch (_) {}
}

function perry_audio_resume_all() {
  const ctx = _perry_audio_ctx();
  if (!ctx) return;
  const p = ctx.resume();
  if (p && typeof p.catch === "function") p.catch(() => {
    if (!PERRY_AUDIO_WARNED_AUTOPLAY) {
      PERRY_AUDIO_WARNED_AUTOPLAY = true;
      console.warn("perry/audio: resumeAll() rejected — must be called from a user gesture.");
    }
  });
}

function perry_audio_is_playing(handle) {
  const kind = _perry_audio_handle_kind(handle);
  if (kind === "playback") {
    const v = _perry_audio_get_playback(handle);
    return (v && !v.stopped && !v.paused) ? 1 : 0;
  }
  if (kind === "sound") {
    const s = _perry_audio_get_sound(handle);
    if (!s) return 0;
    for (const v of PERRY_AUDIO_PLAYBACKS) {
      if (v && v.sound === s && !v.stopped && !v.paused) return 1;
    }
    return 0;
  }
  return 0;
}

function perry_audio_get_duration(soundHandle) {
  const s = _perry_audio_get_sound(soundHandle);
  if (!s || !s.audioBuffer) return 0;
  return s.audioBuffer.duration;
}

function perry_audio_get_position(playbackHandle) {
  const ctx = _perry_audio_ctx();
  if (!ctx) return 0;
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v) return 0;
  if (v.paused) return v.pauseOffset || 0;
  if (v.stopped) return 0;
  const elapsed = ctx.currentTime - v.startedAt;
  if (v.sound && v.sound.audioBuffer) {
    const d = v.sound.audioBuffer.duration;
    if (v.loop && d > 0) return elapsed % d;
    return Math.min(elapsed, d);
  }
  return elapsed;
}

function perry_audio_on_ended(playbackHandle, callback) {
  const v = _perry_audio_get_playback(playbackHandle);
  if (!v) return;
  v.onEnded = (callback === undefined || callback === null) ? null : callback;
}

function perry_audio_on_loaded(soundHandle, callback) {
  const s = _perry_audio_get_sound(soundHandle);
  if (!s) return;
  const cb = (callback === undefined || callback === null) ? null : callback;
  if (s.loaded && cb) {
    // Already loaded — fire on next microtask.
    Promise.resolve().then(() => {
      try { callWasmClosure(cb); }
      catch (err) { console.warn("perry/audio onLoaded threw:", err); }
    });
    return;
  }
  s.onLoaded = cb;
}

// ---------- UI Dispatch table (maps bridge function names to implementations) ----------
const __perryUiDispatch = {
  // Widget creation
  perry_ui_app_create, perry_ui_vstack_create, perry_ui_hstack_create, perry_ui_zstack_create,
  perry_ui_text_create, perry_ui_button_create, perry_ui_textfield_create, perry_ui_securefield_create,
  perry_ui_toggle_create, perry_ui_toggle_set_state, perry_ui_slider_create, perry_ui_scrollview_create, perry_ui_spacer_create,
  perry_ui_divider_create, perry_ui_progressview_create, perry_ui_image_create, perry_ui_picker_create,
  perry_ui_form_create, perry_ui_section_create, perry_ui_navigationstack_create, perry_ui_canvas_create,
  perry_ui_bloomview_create, perry_ui_bloomview_get_hwnd,
  perry_ui_lazyvstack_create, perry_ui_lazyvstack_update, perry_ui_table_create,
  perry_ui_table_set_column_header, perry_ui_table_set_column_width,
  perry_ui_table_update_row_count, perry_ui_table_set_on_row_select, perry_ui_table_get_selected_row,
  perry_ui_textarea_create, perry_ui_textarea_set_string, perry_ui_textarea_get_string,
  perry_ui_vstack_create_with_insets, perry_ui_hstack_create_with_insets,
  // Child management
  perry_ui_widget_add_child, perry_ui_widget_remove_all_children,
  perry_ui_widget_remove_child, perry_ui_widget_reorder_child,
  perry_ui_widget_add_overlay, perry_ui_widget_set_overlay_frame,
  // Styling
  perry_ui_set_background, perry_ui_set_foreground, perry_ui_set_font_size, perry_ui_set_font_weight,
  perry_ui_set_font_family, perry_ui_set_padding, perry_ui_set_frame, perry_ui_set_corner_radius,
  perry_ui_set_border, perry_ui_set_opacity, perry_ui_set_enabled, perry_ui_set_tooltip,
  perry_ui_set_control_size, perry_ui_set_widget_hidden, perry_ui_widget_set_background_gradient,
  perry_ui_widget_set_shadow,
  perry_ui_widget_set_border_color, perry_ui_widget_set_border_width,
  perry_ui_text_set_decoration,
  perry_ui_textfield_set_borderless, perry_ui_stack_set_alignment,
  perry_ui_widget_set_width, perry_ui_widget_set_height, perry_ui_widget_set_hugging,
  perry_ui_widget_match_parent_width, perry_ui_widget_match_parent_height,
  perry_ui_widget_set_edge_insets, perry_ui_stack_set_detaches_hidden, perry_ui_stack_set_distribution,
  perry_ui_widget_set_context_menu,
  // Animations
  perry_ui_animate_opacity, perry_ui_animate_position,
  // #1546: showToast on web
  perry_ui_show_toast,
  // #1865: display-link callbacks (requestAnimationFrame)
  perry_ui_on_frame, perry_ui_cancel_frame,
  // Events
  perry_ui_set_on_click, perry_ui_set_on_hover, perry_ui_set_on_double_click,
  // State
  perry_ui_state_create, perry_ui_state_get, perry_ui_state_set,
  perry_ui_state_on_change, perry_ui_state_bind_text, perry_ui_state_bind_text_numeric,
  perry_ui_state_bind_slider, perry_ui_state_bind_toggle, perry_ui_state_bind_visibility,
  perry_ui_state_bind_foreach, perry_ui_state_bind_textfield,
  // Keyed state + setText (issue #1392 — state_desugar synthetic API)
  perry_ui_set_text, perry_ui_state_init, perry_ui_state_get_keyed, perry_ui_state_set_keyed,
  perry_ui_foreach_register, perry_ui_navstack_register_route,
  // Text/Button/TextField ops
  perry_ui_text_set_string, perry_ui_text_set_selectable, perry_ui_text_set_wraps, perry_ui_text_set_color,
  perry_ui_text_set_text_alignment,
  perry_ui_button_set_bordered, perry_ui_button_set_title, perry_ui_button_set_text_color,
  perry_ui_button_set_image, perry_ui_button_set_content_tint_color, perry_ui_button_set_image_position,
  perry_ui_textfield_focus, perry_ui_textfield_set_string, perry_ui_textfield_get_string,
  perry_ui_textfield_blur_all, perry_ui_textfield_set_on_submit, perry_ui_textfield_set_on_focus,
  // ScrollView
  perry_ui_scrollview_set_child, perry_ui_scrollview_scroll_to,
  perry_ui_scrollview_get_offset, perry_ui_scrollview_set_offset,
  // Canvas
  perry_ui_canvas_fill_rect, perry_ui_canvas_stroke_rect, perry_ui_canvas_clear_rect,
  perry_ui_canvas_set_fill_color, perry_ui_canvas_set_stroke_color,
  perry_ui_canvas_begin_path, perry_ui_canvas_move_to, perry_ui_canvas_line_to,
  perry_ui_canvas_arc, perry_ui_canvas_close_path, perry_ui_canvas_fill, perry_ui_canvas_stroke,
  perry_ui_canvas_set_line_width, perry_ui_canvas_fill_text, perry_ui_canvas_set_font,
    perry_ui_canvas_draw_image,
  perry_ui_canvas_fill_gradient,
  // Navigation
  perry_ui_navstack_push, perry_ui_navstack_pop,
  // Picker
  perry_ui_picker_add_item, perry_ui_picker_set_selected, perry_ui_picker_get_selected,
  // Image
  perry_ui_image_create_symbol, perry_ui_image_create_url,
    perry_ui_load_image, perry_ui_image_set_size, perry_ui_image_set_tint,
  // ProgressView
  perry_ui_progressview_set_value,
  // Menu
  perry_ui_menu_create, perry_ui_menu_add_item, perry_ui_menu_add_item_with_shortcut,
  perry_ui_menu_add_separator, perry_ui_menu_add_submenu, perry_ui_menu_add_standard_action, perry_ui_menu_clear,
  perry_ui_menubar_create, perry_ui_menubar_add_menu, perry_ui_menubar_attach,
  // Clipboard
  perry_ui_clipboard_read, perry_ui_clipboard_write,
  // Dialog
  perry_ui_open_file_dialog, perry_ui_open_folder_dialog, perry_ui_save_file_dialog, perry_ui_alert,
  // Keyboard
  perry_ui_register_global_hotkey, perry_ui_add_keyboard_shortcut,
  perry_ui_widget_set_on_key_down, perry_ui_widget_set_on_key_up,
  perry_ui_app_set_on_key_down, perry_ui_app_set_on_key_up,
  perry_ui_focus_widget, perry_ui_blur_widget, perry_ui_is_key_down,
  perry_ui_current_modifiers,
  // Sheet
  perry_ui_sheet_create, perry_ui_sheet_present, perry_ui_sheet_dismiss,
  // Toolbar
  perry_ui_toolbar_create, perry_ui_toolbar_add_item, perry_ui_toolbar_attach,
  // Window
  perry_ui_window_create, perry_ui_window_set_body, perry_ui_window_show, perry_ui_window_close,
  // App lifecycle
  perry_ui_app_run, perry_ui_app_set_body, perry_ui_app_set_min_size, perry_ui_app_set_max_size,
  perry_ui_app_on_activate, perry_ui_app_on_terminate, perry_ui_app_set_timer,
  // System
  perry_system_open_url, perry_system_is_dark_mode, perry_system_preferences_get,
  perry_system_preferences_set, perry_system_haptic_play,
  perry_system_keychain_save, perry_system_keychain_get,
  perry_system_keychain_delete, perry_system_notification_send,
  // Frame split
  perry_ui_frame_split_create, perry_ui_frame_split_add_child,
  // Camera (issue #191) — browser stubs
  perry_ui_camera_create, perry_ui_camera_start, perry_ui_camera_stop,
  perry_ui_camera_freeze, perry_ui_camera_unfreeze,
  perry_ui_camera_sample_color, perry_ui_camera_set_on_tap,
  // Media (issue #370) — HTML5 <audio> + Media Session API
  perry_media_create_player, perry_media_play, perry_media_pause, perry_media_stop,
  perry_media_seek, perry_media_set_volume, perry_media_set_rate,
  perry_media_get_current_time, perry_media_get_duration, perry_media_get_state,
  perry_media_is_playing, perry_media_on_state_change, perry_media_on_time_update,
  perry_media_set_now_playing, perry_media_destroy,
  // Audio (issue #1867) — Web Audio API: low-latency SFX + buses
  perry_audio_load_sound, perry_audio_unload, perry_audio_play, perry_audio_stop,
  perry_audio_pause, perry_audio_resume, perry_audio_set_volume, perry_audio_set_rate,
  perry_audio_set_pan, perry_audio_fade_in, perry_audio_fade_out, perry_audio_crossfade,
  perry_audio_create_bus, perry_audio_destroy_bus, perry_audio_mute_bus, perry_audio_solo_bus,
  perry_audio_set_master_volume, perry_audio_suspend, perry_audio_resume_all,
  perry_audio_is_playing, perry_audio_get_duration, perry_audio_get_position,
  perry_audio_on_ended, perry_audio_on_loaded,
};

// Also expose as __perryUi for JS async function context
const __perryUi = __perryUiDispatch;

// Unified method dispatch for class_call_N imports.
// Tries: 1) class method table, 2) UI widget methods, 3) state methods.
// When called from mem_call, obj and args are plain JS values (decoded by __bitsToJsValue).
const __uiMethodMap = {
  addChild: "perry_ui_widget_add_child", removeAllChildren: "perry_ui_widget_remove_all_children",
  setBackground: "perry_ui_set_background", setForeground: "perry_ui_set_foreground",
  setFontSize: "perry_ui_set_font_size", setFontWeight: "perry_ui_set_font_weight",
  setFontFamily: "perry_ui_set_font_family", setPadding: "perry_ui_set_padding",
  setFrame: "perry_ui_set_frame", setCornerRadius: "perry_ui_set_corner_radius",
  setBorder: "perry_ui_set_border", setOpacity: "perry_ui_set_opacity",
  setEnabled: "perry_ui_set_enabled", setTooltip: "perry_ui_set_tooltip",
  setControlSize: "perry_ui_set_control_size",
  animateOpacity: "perry_ui_animate_opacity", animatePosition: "perry_ui_animate_position",
  setOnClick: "perry_ui_set_on_click", setOnHover: "perry_ui_set_on_hover",
  setOnDoubleClick: "perry_ui_set_on_double_click",
  get: "perry_ui_state_get", set: "perry_ui_state_set",
  create: "perry_ui_state_create",
  bindText: "perry_ui_state_bind_text", bindTextNumeric: "perry_ui_state_bind_text_numeric",
  bindSlider: "perry_ui_state_bind_slider", bindToggle: "perry_ui_state_bind_toggle",
  bindVisibility: "perry_ui_state_bind_visibility", bindForEach: "perry_ui_state_bind_foreach",
  onChange: "perry_ui_state_on_change",
  setString: "perry_ui_text_set_string", setSelectable: "perry_ui_text_set_selectable",
  setBordered: "perry_ui_button_set_bordered", setTitle: "perry_ui_button_set_title",
  setTextColor: "perry_ui_button_set_text_color", setImage: "perry_ui_button_set_image",
  fillRect: "perry_ui_canvas_fill_rect", strokeRect: "perry_ui_canvas_stroke_rect",
  clearRect: "perry_ui_canvas_clear_rect", setFillColor: "perry_ui_canvas_set_fill_color",
  setStrokeColor: "perry_ui_canvas_set_stroke_color", beginPath: "perry_ui_canvas_begin_path",
  moveTo: "perry_ui_canvas_move_to", lineTo: "perry_ui_canvas_line_to",
  arc: "perry_ui_canvas_arc", closePath: "perry_ui_canvas_close_path",
  fill: "perry_ui_canvas_fill", stroke: "perry_ui_canvas_stroke",
  setLineWidth: "perry_ui_canvas_set_line_width", fillText: "perry_ui_canvas_fill_text",
  setFont: "perry_ui_canvas_set_font", drawImage: "perry_ui_canvas_draw_image",
  setChild: "perry_ui_scrollview_set_child", focus: "perry_ui_textfield_focus",
  setWidth: "perry_ui_widget_set_width", setHeight: "perry_ui_widget_set_height",
  matchParentWidth: "perry_ui_widget_match_parent_width",
  matchParentHeight: "perry_ui_widget_match_parent_height",
  setHidden: "perry_ui_set_widget_hidden",
  setEdgeInsets: "perry_ui_widget_set_edge_insets",
  setShadow: "perry_ui_widget_set_shadow",
  run: "perry_ui_app_run", setBody: "perry_ui_app_set_body",
  addOverlay: "perry_ui_widget_add_overlay",
};

function __classDispatch(objVal, mname, rawArgs) {
  // objVal and rawArgs are already plain JS values (decoded by __bitsToJsValue in mem_call)
  // Primitive/built-in fast-path (Issue #133 item 1): plain strings/numbers have no
  // __class__ and aren't in __uiMethodMap, so method calls like `str.charCodeAt(i)`
  // previously fell through and returned undefined. Route to the native prototype.
  if (typeof objVal === 'string') {
    const fn = String.prototype[mname];
    if (typeof fn === 'function') return fn.apply(objVal, rawArgs);
  } else if (typeof objVal === 'number') {
    const fn = Number.prototype[mname];
    if (typeof fn === 'function') return fn.apply(objVal, rawArgs);
  } else if (Array.isArray(objVal)) {
    // Array methods whose JS-native impls need a plain-JS callback (not a Perry closure
    // handle) are routed through __memDispatch before reaching here — this fallback
    // catches methods the codegen doesn't special-case (at, findLast, flat, etc.).
    const fn = Array.prototype[mname];
    if (typeof fn === 'function') return fn.apply(objVal, rawArgs);
  }
  // 1) Try class method table (for user-defined classes)
  // WASM functions use i64 (BigInt) params/returns
  if (objVal && typeof objVal === 'object' && objVal.__class__) {
    let cls = objVal.__class__;
    while (cls) {
      const methods = classMethodTable[cls];
      if (methods && mname in methods) {
        const fn = wasmInstance?.exports.__indirect_function_table?.get(methods[mname]);
        if (fn) {
          // Pad for omitted TS optional params (Issue #1035).
          const padded = __padBigintArgs(fn, 1, rawArgs.map(v => __jsValueToBits(v)));
          return __bitsToJsValue(fn(__jsValueToBits(objVal), ...padded));
        }
      }
      cls = classParentTable[cls] || null;
    }
  }
  // 2) Try UI widget/state method dispatch
  const uiFnName = __uiMethodMap[mname];
  if (uiFnName) {
    const fn = __perryUiDispatch[uiFnName];
    if (fn) {
      return fn(objVal, ...rawArgs);
    }
  }
  return undefined;
}

// Convert raw u64 BigInt bits to a JS value, decoding NaN-boxed tags directly.
// Never reads NaN-boxed values as f64 (Firefox canonicalizes NaN through Float64Array).
function __bitsToJsValue(bits) {
  if (bits === TAG_UNDEFINED) return undefined;
  if (bits === TAG_NULL) return null;
  if (bits === TAG_TRUE) return true;
  if (bits === TAG_FALSE) return false;
  const tag = bits >> 48n;
  if (tag === STRING_TAG) return stringTable[Number(bits & 0xFFFFFFFFn)];
  if (tag === POINTER_TAG) {
    const obj = handleStore.get(Number(bits & 0xFFFFFFFFn));
    return obj !== undefined ? obj : undefined;
  }
  if (tag === INT32_TAG) {
    // Sign-extend low 32 bits (Issue #133 item 6)
    return Number(BigInt.asIntN(32, bits & 0xFFFFFFFFn));
  }
  // Plain number — safe to read as f64 (not NaN-boxed)
  _u64[0] = bits;
  return _f64[0];
}

// Convert a JS value to raw u64 BigInt bits for writing to WASM memory.
// Never goes through f64 for NaN-boxed values (Firefox canonicalizes NaN).
function __jsValueToBits(v) {
  if (v === undefined) return TAG_UNDEFINED;
  if (v === null) return TAG_NULL;
  if (v === true) return TAG_TRUE;
  if (v === false) return TAG_FALSE;
  if (typeof v === 'number') { _f64[0] = v; return _u64[0]; }
  if (typeof v === 'string') {
    const id = stringTable.length;
    stringTable.push(v);
    return (STRING_TAG << 48n) | BigInt(id);
  }
  // Object/Array/Function → store as handle
  const id = allocHandle(v);
  return (POINTER_TAG << 48n) | BigInt(id);
}

// Boot the WASM module
// ffiImports: optional object mapping FFI function names to JS functions.
// These are injected into the "ffi" WASM import namespace for external native libraries.
async function bootPerryWasm(wasmBase64, ffiImports) {
  if (ffiImports) {
    if (typeof __ffiImports === 'undefined') {
      globalThis.__ffiImports = ffiImports;
    } else {
      Object.assign(__ffiImports, ffiImports);
    }
  }
  const wasmBytes = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));
  const imports = wrapImportsForI64(buildImports());
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  wasmInstance = instance;
  wasmMemory = instance.exports.memory;
  // Call the entry point
  if (instance.exports._start) {
    instance.exports._start();
  } else if (instance.exports.main) {
    instance.exports.main();
  }
}
