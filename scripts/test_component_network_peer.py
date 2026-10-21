import json
from pathlib import Path
import sys
import tempfile
import unittest

from component_network_peer import EXPECTED, exercise

CLIENT = """
import json,socket,sys
with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as s:
    s.settimeout(2)
    for text in json.loads(sys.argv[1]):
        s.sendto(text.encode(),('127.0.0.1',int(sys.argv[3])))
        assert s.recvfrom(65536)[0] == b'reply:'+text.encode()
sys.exit(int(sys.argv[2]))
"""


class PeerTests(unittest.TestCase):
    def test_actual_udp_traffic_is_required_and_denied_payloads_fail(self):
        for payloads, status, passes in [(EXPECTED, 0, True), ([], 0, False),
                (EXPECTED + ['denied'], 0, False), (EXPECTED, 7, False)]:
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
