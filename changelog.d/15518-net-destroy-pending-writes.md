`socket.destroy()` fails the writes and `end()` callbacks still pending with Node's errors: `ERR_SOCKET_CLOSED_BEFORE_CONNECTION` before the connection exists, `write ECANCELED` on an open handle.
