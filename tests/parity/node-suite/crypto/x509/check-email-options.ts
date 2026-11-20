import { X509Certificate } from "node:crypto";

const emailSanPem = `-----BEGIN CERTIFICATE-----
MIIDnTCCAoWgAwIBAgIUUNvkWDKHEHp/+ocqD2MFMOEPGVgwDQYJKoZIhvcNAQEL
BQAwWzELMAkGA1UEBhMCVVMxDjAMBgNVBAoMBVBlcnJ5MRcwFQYDVQQDDA5lbWFp
bC1zYW4udGVzdDEjMCEGCSqGSIb3DQEJARYUc3ViamVjdEBleGFtcGxlLnRlc3Qw
HhcNMjYwNjAzMjMyNjM5WhcNMjcwNjAzMjMyNjM5WjBbMQswCQYDVQQGEwJVUzEO
MAwGA1UECgwFUGVycnkxFzAVBgNVBAMMDmVtYWlsLXNhbi50ZXN0MSMwIQYJKoZI
hvcNAQkBFhRzdWJqZWN0QGV4YW1wbGUudGVzdDCCASIwDQYJKoZIhvcNAQEBBQAD
ggEPADCCAQoCggEBAKIsxn//feq0AfvKrJy5OYhS3QSUJG4JLWnjUffPTVlnKhDy
Hve8Csm4gHqdxKpHdqxB8dsGKEhD8z88xkDxJW7Ehw/XVGBdzbv1/NLXVXETWPVm
xZjyjmywLkZ5vkg+4PpXww5wty7TrCvIz+2AWWrm838Z+LQd0si+U+j77NkRPsFB
co3J15joi8h2o/mbtP6plYe6BwIlT3PJ9Rf9puDF59izWcqGMm+nM9V1MoeL3/Ai
f9F2V3zZu9RSLRNSruv6kEohM+HwTvsNKLUj5ucrzfhsE6xGUAQ9rmg5Fxs5b+Z+
YcFn80TYtVrvLKuKuKo1yi2MjAK12437nVbsUdMCAwEAAaNZMFcwKwYDVR0RBCQw
IoEQc2FuQGV4YW1wbGUudGVzdIIOZW1haWwtc2FuLnRlc3QwCQYDVR0TBAIwADAd
BgNVHQ4EFgQUu1y5db60rsLIfa5hCv2tiF3sUPgwDQYJKoZIhvcNAQELBQADggEB
AHIGCQu7o6+WPuieEqz3gXYlwO9CMBDfQlI47BPwC4jDSnxLlXcI8F2hwdJbloRQ
ZJx0M2IkzSRQvBU+9IBZ0+px1CkHfFwHdvR2ysUl0G6OOplmTqrs9vn8bblEQD+j
43alpVbxOExcg7AM1E2AJMMdz4YIDWJmdV7OErpZT+AtkNwzybpLj/0hlaVE9CT0
P1K2nA/MKJVQrwUjittQckZQrFjoArZvqTsG2VEub07bK/e8FjzoUlSsxrpXYs04
S6LbaoP5VykxlCrc880aXsp0mX0xTWdq7l2ctOTATGXMeoXipKfl63xTgP7mK7pb
xekK5R6VvfyLttVW5BQyjtI=
-----END CERTIFICATE-----`;

const dnsSanOnlyPem = `-----BEGIN CERTIFICATE-----
MIIDkDCCAnigAwIBAgIUI9EOhteL5F7Sgj2O87CSR3x7HVYwDQYJKoZIhvcNAQEL
BQAwXjELMAkGA1UEBhMCVVMxDjAMBgNVBAoMBVBlcnJ5MRYwFAYDVQQDDA1kbnMt
b25seS50ZXN0MScwJQYJKoZIhvcNAQkBFhhkbnMtc3ViamVjdEBleGFtcGxlLnRl
c3QwHhcNMjYwNjAzMjMyNjM5WhcNMjcwNjAzMjMyNjM5WjBeMQswCQYDVQQGEwJV
UzEOMAwGA1UECgwFUGVycnkxFjAUBgNVBAMMDWRucy1vbmx5LnRlc3QxJzAlBgkq
hkiG9w0BCQEWGGRucy1zdWJqZWN0QGV4YW1wbGUudGVzdDCCASIwDQYJKoZIhvcN
AQEBBQADggEPADCCAQoCggEBAKFcPhAY9stifVjXq77TELF0sl0PL3jsuKMNjqL4
3xz9sEHw8s8eMVNYMw3FRdg6wRMpIn/9O3Ia9TNSVddkW08jKVkIJi2QhaiIewZ7
C8oIrImQ3MnSEDiBaqAMQmlANuxYir7su5vdWvoiKgpPVbGcSQYK122I72cH2IRG
BUhN7elR1HCDVQ2LKF01CL3lSWqqPEopoX7lfhyJ6sIZMjARukLvTBsmZyjLK8bO
MSd0yGalJxabO3DQ7jJgBaeycUOif3Pa7Cjmug9R510abKqRzKFZL4tu1vmeHIaD
Kjpt0OgfRQP6okieXadl8xvGo2/9q0fs5UPIBU1bbcatz2MCAwEAAaNGMEQwGAYD
VR0RBBEwD4INZG5zLW9ubHkudGVzdDAJBgNVHRMEAjAAMB0GA1UdDgQWBBSVfSf/
qa6xSuIbu99vxn4DzqBo/jANBgkqhkiG9w0BAQsFAAOCAQEAf+LtTeB1XkpQdBxW
Rl97ieIMo5bnRkZb09E3tM04q7hAWX0oIIRjIqRSdimY/Ff8cVIyGe10slwGt1M+
QKSUEOQB30obKoLwzYdJYoWE9cMuWj2WxD5wSP01GAgFaD8jN5lSTfktZUgxsxVc
tjf1ZD4XU8et3K8KEn9Dqx9zIdcCQEePsZ66HPux4yx3auaV+fo09Og+x698SX3Y
U57bG5zr/2JzKo04ndPIjjk7XSik/fDTiJvwVrYRwynYP5//sq41uUAihrMQqATk
12qh008BtWf0PGAXi53Pa2x0p2CERVKu7YxroRFpKfMaT4afLwxkpl6uDwn0SBOE
JRLcOA==
-----END CERTIFICATE-----`;

const subjectOnlyPem = `-----BEGIN CERTIFICATE-----
MIIDdTCCAl2gAwIBAgIUFRaeJXGId7gEeVJQDgEzKPDUgQ8wDQYJKoZIhvcNAQEL
BQAwYzELMAkGA1UEBhMCVVMxDjAMBgNVBAoMBVBlcnJ5MRowGAYDVQQDDBFzdWJq
ZWN0LW9ubHkudGVzdDEoMCYGCSqGSIb3DQEJARYZc3ViamVjdC1vbmx5QGV4YW1w
bGUudGVzdDAeFw0yNjA2MDMyMzI2MzlaFw0yNzA2MDMyMzI2MzlaMGMxCzAJBgNV
BAYTAlVTMQ4wDAYDVQQKDAVQZXJyeTEaMBgGA1UEAwwRc3ViamVjdC1vbmx5LnRl
c3QxKDAmBgkqhkiG9w0BCQEWGXN1YmplY3Qtb25seUBleGFtcGxlLnRlc3QwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCsSHV+d/wDAAQb7v8p5QlftTn9
3NJDQGUZdsXatMwGEKTZJlMgKmP/jNV9ZyluFR+w3kBsySbKw5265sgOi6TtM0Gs
MD10zc5uLzX2KeeeFSoQ+DWskEnjqdmKGZu0ybYMu6PNmUAr0o1Kqvy63ipsE0lM
lSW10yjnrig9eB5QHmJ0MjXmeH5toxWX2ly9AKNiK9mzMFewcn9Usq1hIdiEBLk8
eD2krcQ3O/s7hz9+uVtaF9Q97d8dhM/qwumnNgq+L+jGD+9Hf+Uz6GSdSn9QWu1I
yoR1JNEiRl6vZ6XdekRgCX+by+TBYZ+tXiwxa3/5XdLCK1QiTJ7+NeJbA58hAgMB
AAGjITAfMB0GA1UdDgQWBBSPujCVXlHBX3Ybbr4HZNSGtMGN+zANBgkqhkiG9w0B
AQsFAAOCAQEAmFTxzLFSv1qINwCV8IH8KMjlpX108w5a0rBm7l2P7vDnqeHvtSsO
ShXxa4R192lODAzx3XjkGIvXVgGpMwGEASEJRWFJC+YKUY0Twu9ca9xk+z2k8tk4
EAu4TbwShqSRWQ5TskJl8umwr6NRt7vkxSAlzGzebGM/mPW0CN5VsozVHtTN8fnc
+ESi37N+H/Xq/xTBkMjDAYJytIpXqSDmQlYlWezZ6eLA2f+k2zFE6ZplwlhUkZYI
lOGpDAXVKStVxFUKZWHHEj3cHui0k0kpYNQbB/vcfV3xpJpaPI4Iw2OFhw0//wHV
IGR/txiRMWJmMlArI/1On502M/NLrv14KA==
-----END CERTIFICATE-----`;

const emailSanCert = new X509Certificate(emailSanPem);
const dnsSanOnlyCert = new X509Certificate(dnsSanOnlyPem);
const subjectOnlyCert = new X509Certificate(subjectOnlyPem);

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "", err.message);
  }
}

report("san email default", () =>
  emailSanCert["checkEmail"]("san@example.test"));
report("san email subject never", () =>
  emailSanCert["checkEmail"]("san@example.test", { subject: "never" }));
report("san subject default blocked", () =>
  emailSanCert["checkEmail"]("subject@example.test"));
report("san subject always", () =>
  emailSanCert["checkEmail"]("subject@example.test", { subject: "always" }));
report("san subject never", () =>
  emailSanCert["checkEmail"]("subject@example.test", { subject: "never" }));
report("dns-only subject default", () =>
  dnsSanOnlyCert["checkEmail"]("dns-subject@example.test"));
report("dns-only subject never", () =>
  dnsSanOnlyCert["checkEmail"]("dns-subject@example.test", { subject: "never" }));
report("subject-only default", () =>
  subjectOnlyCert["checkEmail"]("subject-only@example.test"));
report("subject-only always", () =>
  subjectOnlyCert["checkEmail"]("subject-only@example.test", { subject: "always" }));
report("subject-only never", () =>
  subjectOnlyCert["checkEmail"]("subject-only@example.test", { subject: "never" }));
report("subject case-sensitive miss", () =>
  subjectOnlyCert["checkEmail"]("SUBJECT-ONLY@EXAMPLE.TEST"));
