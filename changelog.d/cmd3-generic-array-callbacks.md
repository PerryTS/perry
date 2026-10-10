Generic Array callback methods now use the same per-operation `DirectCall3/4`
resolution as specialized array methods, removing repeated callback validation
and body classification from each element. The callback is still read from its
existing moving-GC root for every invocation; bound, rest, padded and Proxy
callbacks retain the shared dispatcher.
