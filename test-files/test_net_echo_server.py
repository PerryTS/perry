#!/usr/bin/env python3
"""
Plain-TCP echo server for Perry parity tests.

Listens on 127.0.0.1:17891 and echoes every received byte back to
the same client.  Used by test_net_min.ts and test_net_socket.ts.

Usage (from repo root):
    python3 test-files/test_net_echo_server.py &
    # ... run net tests ...
    kill %1
"""
import socketserver
import sys

PORT = 17891


class EchoHandler(socketserver.BaseRequestHandler):
    def handle(self):
        try:
            while True:
                data = self.request.recv(4096)
                if not data:
                    break
                self.request.sendall(data)
        except (ConnectionResetError, BrokenPipeError):
            pass


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


if __name__ == "__main__":
    try:
        with ReusableTCPServer(("127.0.0.1", PORT), EchoHandler) as server:
            print(f"echo server listening on 127.0.0.1:{PORT}", flush=True)
            server.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)
