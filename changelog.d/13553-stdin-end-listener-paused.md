Registering an `end` listener on `process.stdin` started reading fd 0, and
`readableFlowing`/`isPaused()` did not follow Node. Only data and readable
listeners, resume and pipe start the pump now, and the flowing state tracks
pause and resume.
