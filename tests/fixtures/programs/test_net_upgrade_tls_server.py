#!/usr/bin/env python3
"""
Companion server for test_net_upgrade_tls.ts.

Implements the Postgres-style plain→TLS upgrade (SSLRequest) flow:
  1. Accept a plain TCP connection on port 17892.
  2. Read one byte — the SSLRequest marker sent by the client.
  3. Reply 'S' (server supports SSL, mirroring Postgres's response).
  4. Upgrade the *same* socket to TLS (self-signed cert; client uses verify=0).
  5. Echo all subsequent TLS data back to the client.

Run: python3 test_net_upgrade_tls_server.py [--port PORT]
"""

import argparse
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import threading


def generate_selfsigned_cert(tmpdir: str) -> tuple[str, str]:
    """Generate a temporary self-signed cert+key pair and return (cert_path, key_path)."""
    cert_path = os.path.join(tmpdir, "cert.pem")
    key_path = os.path.join(tmpdir, "key.pem")
    subprocess.run(
        [
            "openssl", "req", "-x509",
            "-newkey", "rsa:2048",
            "-keyout", key_path,
            "-out", cert_path,
            "-days", "1",
            "-nodes",
            "-subj", "/CN=localhost",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return cert_path, key_path


def handle_client(conn: socket.socket, ssl_ctx: ssl.SSLContext) -> None:
    try:
        # 1. Read the SSLRequest marker byte from the client.
        marker = b""
        while len(marker) < 1:
            chunk = conn.recv(1)
            if not chunk:
                return
            marker += chunk

        # 2. Reply 'S' — yes, this server supports SSL.
        conn.sendall(b"S")

        # 3. Upgrade the existing plain socket to TLS (server side).
        tls_conn = ssl_ctx.wrap_socket(conn, server_side=True)

        # 4. Echo loop over TLS.
        try:
            while True:
                data = tls_conn.recv(4096)
                if not data:
                    break
                tls_conn.sendall(data)
            # Send TLS close_notify before closing so the peer (Perry's
            # rustls stack) doesn't see an unexpected EOF.
            try:
                tls_conn.unwrap()
            except (ssl.SSLError, OSError):
                pass
        except ssl.SSLEOFError:
            pass
        finally:
            try:
                tls_conn.close()
            except Exception:
                pass
    except Exception as exc:
        print(f"[tls-upgrade-server] handler error: {exc}", file=sys.stderr, flush=True)
    finally:
        try:
            conn.close()
        except Exception:
            pass


def main() -> None:
    parser = argparse.ArgumentParser(description="plain→TLS upgrade companion server")
    parser.add_argument("--port", type=int, default=17892)
    args = parser.parse_args()

    tmpdir = tempfile.mkdtemp()
    try:
        cert_path, key_path = generate_selfsigned_cert(tmpdir)
    except FileNotFoundError:
        print(
            "[tls-upgrade-server] ERROR: openssl not found — "
            "install openssl to generate self-signed cert",
            file=sys.stderr,
            flush=True,
        )
        sys.exit(1)
    except subprocess.CalledProcessError as exc:
        print(
            f"[tls-upgrade-server] ERROR: openssl cert generation failed: {exc}",
            file=sys.stderr,
            flush=True,
        )
        sys.exit(1)

    ssl_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ssl_ctx.load_cert_chain(cert_path, key_path)

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", args.port))
    srv.listen(5)
    print(
        f"[tls-upgrade-server] listening on 127.0.0.1:{args.port}",
        flush=True,
    )

    while True:
        try:
            conn, _addr = srv.accept()
        except OSError:
            # Socket closed — server shutting down.
            break
        t = threading.Thread(
            target=handle_client, args=(conn, ssl_ctx), daemon=True
        )
        t.start()


if __name__ == "__main__":
    main()
