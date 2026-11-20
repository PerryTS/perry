"""Host-side echo peer for the round-trip fixtures (dgram_rtt, tcp_rtt).

Echoes every UDP datagram and every TCP read on loopback, each DELAY after it
arrives (so the guest has gone idle when the reply lands), prints
"<udp port> <tcp port>" once both are bound, and runs until it is killed. The
WASI runner starts it for a fixture whose config names it as "peer" and
appends the printed ports to the program arguments."""
import socket
import sys
import threading
import time

DELAY = 0.0002


def udp(sock: socket.socket) -> None:
    while True:
        data, addr = sock.recvfrom(65536)
        time.sleep(DELAY)
        sock.sendto(data, addr)


def tcp_conn(conn: socket.socket) -> None:
    conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    with conn:
        while data := conn.recv(65536):
            time.sleep(DELAY)
            conn.sendall(data)


def tcp(listener: socket.socket) -> None:
    while True:
        conn, _ = listener.accept()
        threading.Thread(target=tcp_conn, args=(conn,), daemon=True).start()


def main() -> None:
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    u.bind(("127.0.0.1", 0))
    t = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    t.bind(("127.0.0.1", 0))
    t.listen(8)
    threading.Thread(target=udp, args=(u,), daemon=True).start()
    threading.Thread(target=tcp, args=(t,), daemon=True).start()
    print(u.getsockname()[1], t.getsockname()[1], flush=True)
    threading.Event().wait()


if __name__ == "__main__":
    main()
