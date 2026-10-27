import { cache as __perry_cjs_cache, main as __perry_cjs_main, extensions as __perry_cjs_extensions, setPending as __perry_cjs_set_pending, takePending as __perry_cjs_take_pending, link as __perry_cjs_link, requireComputed as __perry_cjs_require_computed, requireError as __perry_cjs_require_error, reload as __perry_cjs_reload } from 'perry:cjs';
import { createRequire as __perry_cjs_create_require } from 'node:module';

export var __perry_cjs_record;


class DictEntry {
  constructor(hash, key, value, next) {
    this.hash = hash;
    this.key = key;
    this.value = value;
    this.next = next;
  }

  match(hash, key) {
    return this.hash === hash && key === this.key;
  }
}
class DictIdEntry extends DictEntry {
  match(hash, key) {
    return this.hash === hash && this.key === key;
  }
}
class Random {
  constructor() {
    this.seed = 74755;
  }

  next() {
    this.seed = (this.seed * 1309 + 13849) & 65535;
    return this.seed;
  }
}
const _cjs = (function() {
function __perry_cjs_factory() {
    // #3527: `module`/`exports` are reassignable `var`s (mirroring Node, where
    // they are wrapper-function parameters), so CJS bodies that do
    // `var module = X` / `module = X` / `exports = X` — e.g. iconv-lite's
    // `for (...) { var module = modules[i]; mergeModules(exports, module); }`
    // — rebind the local instead of colliding with a `const`. The stable
    // `__cjs_module` is what the module actually exports, read back at the end;
    // a body reassigning its local `module` can't clobber it (Node holds the
    // real module ref the same way), so named/default-export resolution stays
    // correct regardless of what the body does to its `module` local.
    // #6769: the Node `Module` record surface. Set before user code so a
    // recursive load of this module observes the same shape Node exposes.
    const __cjs_module = {
        exports: __perry_declared_keys_object("Set", "IdentitySet", "Dictionary", "IdentityDictionary", "Vector", "Random"),
        __perry_cjs_record: true,
        id: "/t6/som.js",
        path: "/t6",
        filename: "/t6/som.js",
        loaded: false,
        children: [],
        parent: undefined,
        paths: ["/t6" + '/node_modules'],
        require: undefined
    };
    // The module's record binding: what a static edge of a requiring module
    // reads, from before the body, so a cycle sees the partial record.
    __perry_cjs_record = __cjs_module;
    
        __cjs_module.parent = __perry_cjs_take_pending();
    
    
        // Node populates `module.parent` before the body evaluates, so link
        // it here rather than at the tail's registry publication.
        __perry_link_path_module_parent(__cjs_module);
        // Publish the record before user code, just as at final publication.
        // #10178: esbuild replaces module.exports before requiring its peer.
        // Holding the initial empty object loses that replacement at
        // re-entry; the runtime's record unwrap reads the current exports.
        // Only a program that can require by path or reload (not closed)
        // has a path registry; the factory is held by nobody otherwise.
        __perry_register_path_module_partial("/t6/som.js", __cjs_module, __perry_cjs_factory);
    
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // The program's `require.cache` (the support module's object, whose keys
    // are the graph's module paths): this module's key is a slot.
    const __perry_cjs_c = __perry_cjs_cache;
    __perry_cjs_c["/t6/som.js"] = __cjs_module;

    // This module's own loader for the cold paths of `require` (a computed
    // specifier outside the graph, relative to this module), created on
    // first use: `createRequire` costs ~117k instructions. Only a program
    // that is not closed can compute a specifier or reload.
    let __perry_cjs_reload_require;
    function __perry_cjs_reloader() {
        
            if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/t6/som.js");
            return __perry_cjs_reload_require;
        
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');

        // A specifier no case above resolved: builtins, native addons, a
        // compiled module reached by a computed path, a `.json` file, a
        // package outside the graph (`perry:cjs`'s `requireComputed`).
        // A closed program has no computed require and no reload (design
        // CJS_LINK §2.5): no loader surface is linked for it.
        
            return __perry_cjs_require_computed(specifier, "/t6", __perry_cjs_reloader);
        
    }
    // No `defineProperty(require, 'name', ...)`: a `function require(...)`
    // declaration already carries exactly
    // {value:'require', writable:false, enumerable:false, configurable:true},
    // verified identical in Node 26 and Perry. The redundant install also gave
    // the require object OBJ_FLAG_HAS_DESCRIPTORS, which pushed every later
    // `require.resolve = ...` / `require.cache = ...` assignment onto the
    // descriptor-bearing store path (#10287) in every CommonJS module.
    const __perry_cjs_resolve = function resolve(specifier, options) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');

        // `require.resolve` is reachable only through `require` as a value,
        // which a closed program does not have.
        
            if (__perry_is_builtin(specifier)) return specifier;
            var __perry_nm_resolved = __perry_require_resolve_node_modules("/t6", specifier);
            if (__perry_nm_resolved !== undefined) return __perry_nm_resolved;
        
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    };
    __perry_cjs_resolve.paths = function paths(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    };
    require.resolve = __perry_cjs_resolve;
    // `cache`, `extensions` and `main` are the program's (`perry:cjs`): Node's
    // loader reads `Module._cache`, never this property, so reassigning
    // `require.cache` does not redirect loading.
    require.cache = __perry_cjs_c;
    require.extensions = __perry_cjs_extensions;
    require.main = __perry_cjs_main;
    module.require = function moduleRequire(specifier) { return require(specifier); };

    // @ts-check
// This code is derived from the SOM benchmarks, see AUTHORS.md file.
//
// Copyright (c) 2015-2016 Stefan Marr <git@stefan-marr.de>
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the 'Software'), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED 'AS IS', WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

const INITIAL_SIZE = 10;
const INITIAL_CAPACITY = 16;

class Vector {
  constructor(size) {
    this.storage = size === undefined || size === 0 ? null : new Array(size);
    this.firstIdx = 0;
    this.lastIdx = 0;
  }

  static with(elem) {
    const v = new Vector(1);
    v.append(elem);
    return v;
  }

  at(idx) {
    if (this.storage === null || idx >= this.storage.length) {
      return null;
    }
    return this.storage[idx];
  }

  atPut(idx, val) {
    if (this.storage === null) {
      this.storage = new Array(Math.max(idx + 1, INITIAL_SIZE));
    } else if (idx >= this.storage.length) {
      let newLength = this.storage.length;
      while (newLength <= idx) {
        newLength *= 2;
      }
      this.storage = this.storage.slice();
      this.storage.length = newLength;
    }
    this.storage[idx] = val;
    if (this.lastIdx < idx + 1) {
      this.lastIdx = idx + 1;
    }
  }

  append(elem) {
    if (this.storage === null) {
      this.storage = new Array(INITIAL_SIZE);
    } else if (this.lastIdx >= this.storage.length) {
      // Copy storage to comply with rules, but don't extend storage
      const newLength = this.storage.length * 2;
      this.storage = this.storage.slice();
      this.storage.length = newLength;
    }

    this.storage[this.lastIdx] = elem;
    this.lastIdx += 1;
  }

  isEmpty() {
    return this.lastIdx === this.firstIdx;
  }

  forEach(fn) {
    for (let i = this.firstIdx; i < this.lastIdx; i += 1) {
      fn(this.storage[i]);
    }
  }

  hasSome(fn) {
    for (let i = this.firstIdx; i < this.lastIdx; i += 1) {
      if (fn(this.storage[i])) {
        return true;
      }
    }
    return false;
  }

  getOne(fn) {
    for (let i = this.firstIdx; i < this.lastIdx; i += 1) {
      const e = this.storage[i];
      if (fn(e)) {
        return e;
      }
    }
    return null;
  }

  removeFirst() {
    if (this.isEmpty()) {
      return null;
    }
    this.firstIdx += 1;
    return this.storage[this.firstIdx - 1];
  }

  remove(obj) {
    if (this.storage === null || this.isEmpty()) {
      return false;
    }

    const newArray = new Array(this.capacity());
    let newLast = 0;
    let found = false;

    this.forEach((it) => {
      if (it === obj) {
        found = true;
      } else {
        newArray[newLast] = it;
        newLast += 1;
      }
    });

    this.storage = newArray;
    this.lastIdx = newLast;
    this.firstIdx = 0;
    return found;
  }

  removeAll() {
    this.firstIdx = 0;
    this.lastIdx = 0;
    if (this.storage !== null) {
      this.storage = new Array(this.storage.length);
    }
  }

  size() {
    return this.lastIdx - this.firstIdx;
  }

  capacity() {
    return this.storage === null ? 0 : this.storage.length;
  }

  // eslint-disable-next-line no-unused-vars
  swap(storage, i, j) {
    throw new Error('Not Implemented');
  }

  // eslint-disable-next-line no-unused-vars
  defaultSort(i, j) {
    throw new Error('Not Implemented');
  }

  sortRange(i, j, compare) {
    if (!compare) {
      this.defaultSort(i, j);
    }

    const n = j + 1 - i;
    if (n <= 1) {
      return;
    }

    let di = this.storage[i];
    let dj = this.storage[j];

    if (compare(di, dj)) {
      this.swap(this.storage, i, j);
      const tt = di;
      di = dj;
      dj = tt;
    }

    if (n > 2) {
      const ij = (i + j) / 2;
      let dij = this.storage[ij];

      if (compare(di, dij) <= 0) {
        if (!compare(dij, dj)) {
          this.swap(this.storage, j, ij);
          dij = dj;
        }
      } else {
        this.swap(this.storage, i, ij);
        dij = di;
      }

      if (n > 3) {
        let k = i;
        let l = j - 1;

        // eslint-disable-next-line no-constant-condition
        while (true) {
          while (k <= l && compare(dij, this.storage[l])) {
            l -= 1;
          }

          k += 1;
          while (k <= l && compare(this.storage[k], dij)) {
            k += 1;
          }

          if (k > l) {
            break;
          }
          this.swap(this.storage, k, l);
        }

        const c = null; // never used
        this.sort(i, l, c);
        this.sort(k, j, c);
      }
    }
  }

  // eslint-disable-next-line no-unused-vars
  sort(compare, i, c) {
    if (this.size() > 0) {
      this.sortRange(this.firstIdx, this.lastIdx - 1, compare);
    }
  }
}

class Set {
  constructor(size) {
    this.items = new Vector(size === undefined ? INITIAL_SIZE : size);
  }

  size() {
    return this.items.size();
  }

  forEach(fn) {
    this.items.forEach(fn);
  }

  hasSome(fn) {
    return this.items.hasSome(fn);
  }

  getOne(fn) {
    return this.items.getOne(fn);
  }

  add(obj) {
    if (!this.contains(obj)) {
      this.items.append(obj);
    }
  }

  contains(obj) {
    return this.hasSome((e) => e === obj);
  }

  removeAll() {
    this.items.removeAll();
  }

  collect(fn) {
    const coll = new Vector();
    this.forEach((e) => coll.append(fn(e)));
    return coll;
  }
}

class IdentitySet extends Set {
  constructor(size) {
    super(size === undefined ? INITIAL_SIZE : size);
  }

  contains(obj) {
    return this.hasSome((e) => e === obj);
  }
}

                 
                                       
                     
                   
                       
                     
   

                    
                                                  
   
 

function hashFn(key) {
  if (!key) {
    return 0;
  }
  const hash = key.customHash();
  return hash ^ (hash >>> 16);
}

class Dictionary {
  constructor(size) {
    this.buckets = new Array(size === undefined ? INITIAL_CAPACITY : size);
    this.size_ = 0;
  }

  size() {
    return this.size_;
  }

  isEmpty() {
    return this.size_ === 0;
  }

  getBucketIdx(hash) {
    return (this.buckets.length - 1) & hash;
  }

  getBucket(hash) {
    return this.buckets[this.getBucketIdx(hash)];
  }

  at(key) {
    const hash_ = hashFn(key);
    let e = this.getBucket(hash_);

    while (e) {
      if (e.match(hash_, key)) {
        return e.value;
      }
      e = e.next;
    }
    return null;
  }

  containsKey(key) {
    const hash_ = hashFn(key);
    let e = this.getBucket(hash_);

    while (e) {
      if (e.match(hash_, key)) {
        return true;
      }
      e = e.next;
    }
    return false;
  }

  atPut(key, value) {
    const hash_ = hashFn(key);
    const i = this.getBucketIdx(hash_);
    const current = this.buckets[i];

    if (!current) {
      this.buckets[i] = this.newEntry(key, value, hash_);
      this.size_ += 1;
    } else {
      this.insertBucketEntry(key, value, hash_, current);
    }

    if (this.size_ > this.buckets.length) {
      this.resize();
    }
  }

  newEntry(key, value, hash) {
    return new DictEntry(hash, key, value, null);
  }

  insertBucketEntry(key, value, hash, head) {
    let current = head;

    // eslint-disable-next-line no-constant-condition
    while (true) {
      if (current.match(hash, key)) {
        current.value = value;
        return;
      }
      if (!current.next) {
        this.size_ += 1;
        current.next = this.newEntry(key, value, hash);
        return;
      }
      current = current.next;
    }
  }

  resize() {
    const oldStorage = this.buckets;
    this.buckets = new Array(oldStorage.length * 2);
    this.transferEntries(oldStorage);
  }

  transferEntries(oldStorage) {
    for (let i = 0; i < oldStorage.length; i += 1) {
      const current = oldStorage[i];
      if (current) {
        oldStorage[i] = null;

        if (!current.next) {
          this.buckets[current.hash & (this.buckets.length - 1)] = current;
        } else {
          this.splitBucket(oldStorage, i, current);
        }
      }
    }
  }

  splitBucket(oldStorage, i, head) {
    let loHead = null;
    let loTail = null;
    let hiHead = null;
    let hiTail = null;
    let current = head;

    while (current) {
      if ((current.hash & oldStorage.length) === 0) {
        if (!loTail) {
          loHead = current;
        } else {
          loTail.next = current;
        }
        loTail = current;
      } else {
        if (!hiTail) {
          hiHead = current;
        } else {
          hiTail.next = current;
        }
        hiTail = current;
      }
      current = current.next;
    }

    if (loTail) {
      loTail.next = null;
      this.buckets[i] = loHead;
    }
    if (hiTail) {
      hiTail.next = null;
      this.buckets[i + oldStorage.length] = hiHead;
    }
  }

  removeAll() {
    this.buckets = new Array(this.buckets.length);
    this.size_ = 0;
  }

  getKeys() {
    const keys = new Vector(this.size_);
    for (let i = 0; i < this.buckets.length; i += 1) {
      let current = this.buckets[i];
      while (current) {
        keys.append(current.key);
        current = current.next;
      }
    }
    return keys;
  }

  getValues() {
    const values = new Vector(this.size_);
    for (let i = 0; i < this.buckets.length; i += 1) {
      let current = this.buckets[i];
      while (current) {
        values.append(current.value);
        current = current.next;
      }
    }
    return values;
  }
}

                                     
                    
                                                  
   
 

class IdentityDictionary extends Dictionary {
  constructor(size) {
    super(size === undefined ? INITIAL_CAPACITY : size);
  }

  newEntry(key, value, hash) {
    return new DictIdEntry(hash, key, value, null);
  }
}

              
                 
                      
   

          
                                                   
                     
   
 

exports.Set = Set;
exports.IdentitySet = IdentitySet;
exports.Dictionary = Dictionary;
exports.IdentityDictionary = IdentityDictionary;
exports.Vector = Vector;
exports.Random = Random;


    __cjs_module.loaded = true;  __perry_register_path_module("/t6/som.js", __cjs_module); 
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;
export { DictEntry };
export { DictIdEntry };
export { Random };

export const Set = _cjs.Set;
export const IdentitySet = _cjs.IdentitySet;
export const Dictionary = _cjs.Dictionary;
export const IdentityDictionary = _cjs.IdentityDictionary;
export const Vector = _cjs.Vector;

