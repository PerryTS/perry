`http2`: `http2.connect(authority, { servername })` sends `servername` as SNI and checks the server's certificate against it, instead of the authority's host (#15313).
