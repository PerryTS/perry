`Readable.from(array)` no longer empties the caller's array. The stream
queued the array itself and shifted chunks off it as they were read, so a
second `Readable.from(arr)` saw nothing and `arr.length` was 0 afterwards. It
now queues a copy, as Node only iterates the array.
