import json
from pathlib import Path
import sys
import tempfile
import unittest

from component_network_peer import EXPECTED, EXPECTED_TCP, exercise

CLIENT = """
import json,socket,sys
payloads = json.loads(sys.argv[1])
with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as s:
    s.settimeout(2)
    for text in payloads["udp"]:
        s.sendto(text.encode(),('127.0.0.1',int(sys.argv[3])))
        assert s.recvfrom(65536)[0] == b'reply:'+text.encode()
for text in payloads["tcp"]:
    with socket.create_connection(('127.0.0.1',int(sys.argv[4])),timeout=2) as s:
        data = text.encode()+bytes([10])
        s.sendall(data)
        response = b''
        while not response.endswith(bytes([10])):
            chunk = s.recv(256)
            assert chunk
            response += chunk
        assert response == b'reply:'+data
sys.exit(int(sys.argv[2]))
"""


class PeerTests(unittest.TestCase):
    def test_actual_traffic_is_required_and_denied_payloads_fail(self):
        good = {"udp": EXPECTED, "tcp": EXPECTED_TCP}
        for payloads, status, passes in [(good, 0, True), ({"udp": [], "tcp": EXPECTED_TCP}, 0, False),
                ({"udp": EXPECTED, "tcp": []}, 0, False),
                ({"udp": EXPECTED + ['denied'], "tcp": EXPECTED_TCP}, 0, False),
                ({"udp": EXPECTED, "tcp": EXPECTED_TCP + ['denied']}, 0, False), (good, 7, False)]:
            with self.subTest(payloads=payloads, status=status), tempfile.TemporaryDirectory() as d:
                report = Path(d) / 'peer.json'
                result = exercise([sys.executable, '-c', CLIENT, json.dumps(payloads), str(status)], report, 10)
                self.assertEqual(result == 0, passes)
                self.assertEqual(json.loads(report.read_text())['passed'], passes)

    def test_timed_out_host_cannot_pass(self):
        with tempfile.TemporaryDirectory() as d:
            report = Path(d) / 'peer.json'
            result = exercise([sys.executable, '-c', 'import time; time.sleep(5)'], report, .1)
            self.assertNotEqual(result, 0)
            self.assertEqual(json.loads(report.read_text())['hostExit'], 124)


if __name__ == '__main__':
    unittest.main()
