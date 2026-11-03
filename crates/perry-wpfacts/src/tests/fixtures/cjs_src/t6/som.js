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

class DictIdEntry extends DictEntry {
  match(hash, key) {
    return this.hash === hash && this.key === key;
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

class Random {
  constructor() {
    this.seed = 74755;
  }

  next() {
    this.seed = (this.seed * 1309 + 13849) & 65535;
    return this.seed;
  }
}

exports.Set = Set;
exports.IdentitySet = IdentitySet;
exports.Dictionary = Dictionary;
exports.IdentityDictionary = IdentityDictionary;
exports.Vector = Vector;
exports.Random = Random;
