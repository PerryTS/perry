Zlib streams now produce output in `chunkSize` chunks (16 KiB by default)
and suspend codec work at the readable high-water mark until their consumer
drains the queue. Async iteration pauses the native handle between pulls;
slow pipe destinations resume it on drain. Writes, late consumers, finish/end
callbacks, iterator return and destroy retain their stream ordering while
completed or destroyed codecs release their input and output storage.

Regression tests cover bounded chunks, native pending capacity, suspended writes,
late consumers, pause/resume, slow pipes, iterator return/destroy and finish/end
ordering.
