import { X509Certificate } from "node:crypto";

const sanPem = `-----BEGIN CERTIFICATE-----
MIIDQTCCAimgAwIBAgIUdM+DrVg0qioOfxnnJUQ14DHSQcAwDQYJKoZIhvcNAQEL
BQAwHjEcMBoGA1UEAwwTdW51c2VkLXN1YmplY3QudGVzdDAeFw0yNjA2MDMyMzAz
MjhaFw0yNzA2MDMyMzAzMjhaMB4xHDAaBgNVBAMME3VudXNlZC1zdWJqZWN0LnRl
c3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCyPe7y2KXjojSt4kCi
Cw4uLwKbM4P9pfG37XfJpPkMjOloI8dTKOlw5wUokg5wDYjZg3+4etfj3RyMyweo
168rkbXZHC21v5zw34FWBthCj3bLl/B2KU76Ki0DrFSQ3XHmuMTzYieLhryewQfC
5B2u7TINIuugFWXGh/9qB3WqJn4XAJ3V4POhy9sak52Cggb8gUKVOdm1rqiXP5Xa
TioaTJqsXNrM7vJ01CioL50uBdkzrokqeZASd1/Ouo+iELPLARjmdgY1t0qsus4I
cYsHYk6SjBQspUSut6WY1bU6DjaeiD7RaIhET1RUe8OC+uagIoMimNs1xHJlJzkt
M/03AgMBAAGjdzB1MFQGA1UdEQRNMEuCDiouZXhhbXBsZS50ZXN0gg9mKi5wYXJ0
aWFsLnRlc3SCFCoubXVsdGkuZXhhbXBsZS50ZXN0ghJleGFjdC5leGFtcGxlLnRl
c3QwHQYDVR0OBBYEFMrzNiJDY7M2fpUFtLH5xvt9bHDdMA0GCSqGSIb3DQEBCwUA
A4IBAQAbV7JZllcIGWs/QZIo6f3yVyA4dzmyKEG+7PIS+SPUi5h/R9+6h5+Ess1E
7+n+IW5EP8UNq4kFJaoQRwNWRxiELUghxJKe96dsGah3NnkADTJv9hksw9x2GEtb
0r9R20cFOb6pQxL/bvNHXWUkac+OSWUeG0/7ya/lhXKwy82BP0dC7bmsPyOmc9VF
fDesTnn9TS09mCm5dIvrqx6Cv+oIyzsjm7bx4tWYGKr655r22Grmftr2hj0JQpjH
sTpZDRKnqL+RSjqPykoAaHVgEOz87TwLbtnmxbTRVjaxC2nwm+08gQnvabO4PS6z
JNYX4btmoYsWnMu+LjpBMYBf/TKz
-----END CERTIFICATE-----`;

const cnOnlyPem = `-----BEGIN CERTIFICATE-----
MIIC4TCCAcmgAwIBAgIUT2Pmm498Rc/QLZlHprJ7h7ZJYoEwDQYJKoZIhvcNAQEL
BQAwGTEXMBUGA1UEAwwOKi5jbi1vbmx5LnRlc3QwHhcNMjYwNjAzMjMwMzI4WhcN
MjcwNjAzMjMwMzI4WjAZMRcwFQYDVQQDDA4qLmNuLW9ubHkudGVzdDCCASIwDQYJ
KoZIhvcNAQEBBQADggEPADCCAQoCggEBALv2ANRBWJHb71fTsCtCvUytHVmWrcz/
0xTvNRFeStefuo1meXNljz6FkTiosqD2trmCA5SHfI11szHNH5c0P+8mTqzK/Z31
t7c50SFQthyQv8uboUAqkh1ZLmsL7O0SHmAOd2GG7HxdV2tkEDGjIyx0OVnKTQC3
WDYtfO0STDWo2UUBcBygbHvoJcIEd0G3e69oXXfspzNx4HaYXXDisbEWgpiKruiA
6Oiifa/MCrDqhw3MImCo/a8ibZva5t0zhuSIpiKICFYSdspiRts4NAQ50O5INCtQ
UgSxcT+zvyRxzyzLKBV8YmXI0VBrwZhyUHTIGis+6w9g6mYyHvYwTNECAwEAAaMh
MB8wHQYDVR0OBBYEFIKyDAzP63MS4IJWvbkgJRZ8LiaYMA0GCSqGSIb3DQEBCwUA
A4IBAQA+md3YqLinuVUbjUy9g9CVG9z4bChWsvvu9LL90RoM9UsN/hcdG2I0ra1O
gLBYJFR8+z16k0MIaQTvmFjn6NbCwLuemVsG481IUbdR+imIcZJCvQ8vPm9qPZHp
8zwiJpTbqlXdyc34Je7C7PtTBWfnVc2Kza4F+1TPbiBxNg1j2Xv204BnYV/ymy81
pJ64kX2jHiglIGoNtroRW9XJtgT46IZKnBixyrvXI1LyQ4yRn/sW3zylzv2wRZ6+
OgOV6qEP9MI/nkRxjjOhgd6OhtEw7Oii6WZe3EU4LoJRMGmM6o6gxT0pSmzUAtn9
rwGNC7scODmtWIlD3dV3oVLxqAaD
-----END CERTIFICATE-----`;

const sanCert = new X509Certificate(sanPem);
const cnOnlyCert = new X509Certificate(cnOnlyPem);

function show(label: string, value: unknown) {
  console.log(`${label}:`, value);
}

show("exact san", sanCert["checkHost"]("exact.example.test"));
show("wildcard default", sanCert["checkHost"]("www.example.test"));
show("wildcard false", sanCert["checkHost"]("www.example.test", { wildcards: false }));
show("partial default", sanCert["checkHost"]("foo.partial.test"));
show(
  "partial false",
  sanCert["checkHost"]("foo.partial.test", { partialWildcards: false }),
);
show("multi default", sanCert["checkHost"]("a.b.multi.example.test"));
show(
  "multi true",
  sanCert["checkHost"]("a.b.multi.example.test", { multiLabelWildcards: true }),
);
show("subject default blocked", sanCert["checkHost"]("unused-subject.test"));
show(
  "subject always",
  sanCert["checkHost"]("unused-subject.test", { subject: "always" }),
);
show(
  "subject never exact san",
  sanCert["checkHost"]("exact.example.test", { subject: "never" }),
);
show("cn wildcard default", cnOnlyCert["checkHost"]("www.cn-only.test"));
show(
  "cn wildcard false",
  cnOnlyCert["checkHost"]("www.cn-only.test", { wildcards: false }),
);
show(
  "cn subject never",
  cnOnlyCert["checkHost"]("www.cn-only.test", { subject: "never" }),
);
