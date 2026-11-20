import {
  X509Certificate,
  createPrivateKey,
  createPublicKey,
  createSecretKey,
} from "node:crypto";

const certPem = `-----BEGIN CERTIFICATE-----
MIIDJTCCAg2gAwIBAgIUaMxORuQo1GE1YB4X0JyXV5+QyLYwDQYJKoZIhvcNAQEL
BQAwIjEgMB4GA1UEAwwXcGVycnktY2hlY2stcHJpdmF0ZS1rZXkwHhcNMjYwNjAz
MjEyNzExWhcNMjcwNjAzMjEyNzExWjAiMSAwHgYDVQQDDBdwZXJyeS1jaGVjay1w
cml2YXRlLWtleTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKXLyH7X
dord0t4IjFy0T2/qR4mkZK4jtIs+Nwh8QuKi8egpGdsjOTVMXz3/n9QFs4J9SRLw
dkP7e0JBh6KRbcJYm+JrML1rHPKg8xfoTxDlHmhaV1IWgSAhOEOb9vW9zKJX7TNv
4rrcLAL7f96GNkdmtpJ87x+uyGmLSBu0dzp0LXCXfObNaE7L8piaItPs96chesLD
V3zD+nTMbASD2UAykGdP2HfZ/Xymn+a6A2jzjAoRpSqPVrGndzIbDKBEzK4A5l2/
qBA0b91flxgicjyD2WBZDbNCf1Ha35El908XMAjE4ybw4zT0sRtnzgf34nHvVf6o
93cqSnYks3wafasCAwEAAaNTMFEwHQYDVR0OBBYEFOzo1OmXkR/IdelThccEsUWM
3cUHMB8GA1UdIwQYMBaAFOzo1OmXkR/IdelThccEsUWM3cUHMA8GA1UdEwEB/wQF
MAMBAf8wDQYJKoZIhvcNAQELBQADggEBAH/hIHVvB8JEhXmGjTSPu+4x12umPTTX
NSDhYvvQ6DPR+P/Id3y8V5QKiMrKdT4RasrRDalLgoqcicUpXutFgm07krkiY+RZ
6luvV3EwyWOPjj6Hu6VQ5DTyQAJGNI52UIico5ZrKHswXIdr7BRBHiRpK7FQaXPy
p6PMilT1x+Zyw0v3p7nonWIf+OnKkeW8Dhz/al6qDWLDr1Q57rZsvN0oN7720cCy
AtnD73wFNNU/PJVDa2JTv0mihRbSqZYiDq4V7qIX2HPRTbEBc3NsLYn4WPwE+01f
QdMcKC/JQOCYA6+Ylbwjg41IxxO9TYzmvu2DCsiJMT6l4MHxV1JOrYY=
-----END CERTIFICATE-----`;

const matchingPrivatePem = `-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCly8h+13aK3dLe
CIxctE9v6keJpGSuI7SLPjcIfELiovHoKRnbIzk1TF89/5/UBbOCfUkS8HZD+3tC
QYeikW3CWJviazC9axzyoPMX6E8Q5R5oWldSFoEgIThDm/b1vcyiV+0zb+K63CwC
+3/ehjZHZraSfO8frshpi0gbtHc6dC1wl3zmzWhOy/KYmiLT7PenIXrCw1d8w/p0
zGwEg9lAMpBnT9h32f18pp/mugNo84wKEaUqj1axp3cyGwygRMyuAOZdv6gQNG/d
X5cYInI8g9lgWQ2zQn9R2t+RJfdPFzAIxOMm8OM09LEbZ84H9+Jx71X+qPd3Kkp2
JLN8Gn2rAgMBAAECggEABapeihMTznABFixFm59fvYvMcQQompjGwSFZoRUZ9gOq
b4wEAayE9nDLKmOzUvv04+cjGZ4U9ILB9gQmPeRpU0RS41xVWIux/AqK9AywsvuZ
W+iGZlw1gmMQOKM6P7CCLyQBC4ptvYPrjxiICJMehLcaUwwo4bTHzW+Agc3bayhi
FosFLUuwmtEMKgzb7PVfGccH5XrPD/N1bJeztLz7svzXP2tAbLBc1l7jjHbnD/I5
K/cRhplGy2q0/xOP5VFUUnOQTuJUIkDWTXzLibT2RgnTxmkRjHTRDTmre2CAZB2w
kYXwN5SQPDM0tBxw7oWNQY7c8xc9cvMctTH5/fJMGQKBgQDPL8edqtqmowtQo5PT
fAeYmt/Rsxtuq+tDxB/6mQtJ3kigh/BnpLXmruktPAT2r4IT/gXpeZtLCIvMQvZJ
mZdaI1HV80h4tToaCMdee8Pk+6p1Usbu33I/zo21Gjl6WKvDYa3/9b5eWatKWQxN
s88BjpiTXJDl0s//Ocxg+wqmKQKBgQDM25O4GocHkk0CT3Exlgwds7UYSOcBIme7
MxaaosakTgTjNiKWf0LHVKiYaAMfCA6E4rHitZsCYReBWYDmXKvGINg+X/2OSBAM
So15FQZc5k2cCdD/C8jCgj9PfvwPnEam+/b3mDeDOeLRbrjjf3xLPfld/Mc/DxzG
xWITEfm3swKBgANzxFu4MRR9uv6I+zmW43mDex8/YMGjU7Q5XF8MlceRUJx8J2FS
uUUyvOfoDB0gJ4a1wNt3D0NczReGNhxb1s3FsONjvl1kh6dPZiMI5Oa32stBqdbp
Gjo98taFrVeAirwisIeHTLi9vcDrYu0YheZ8vcYW0MNDk/uotuMWy8KhAoGADKgQ
Q1KYPxaB3X+s/aRIkVk1+g8e/onyoLUyU1F1Nld/o84HawbnyErps6jRcIxd4UXk
OZ6Aui/ndN1jwle9YRtMYOYrUywOmcPNY8qxvvGXn+lXWTqQJ7xGTxIIXqqIDu8I
PhnQbDIaWlgd4ihRNJDapDzmznWPkJRHT+hPZlMCgYA1EJzs1y+bEkRgyh44eGop
xB4WLkP9Fv1z0Q/cRHrVS4DBiO2nqsh5jBux59eGR6HdHH9kZ4hvyqLg+KgoFZaU
TbwGGgfn0yeoyy5wni25k4wIugYueWWXzQYM0WijTIliDTipEfMRmNc3RZsq3GKA
dyEbgft/SQsR7nBGwoxl0A==
-----END PRIVATE KEY-----`;

const otherPrivatePem = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC5U6mL6SiGtANk
mAp7DoyZZrBeaCb4jqn8bAXAvXLrUsGnHY/YfFpIs0BgMI8+mpj/HJt48KUMW1h/
z2/3SXkU2TS7WqZXqsE7ycMuTctZJaxXaOnzvzAqa5cP+DzDUE6mTIpz8o7WEVBd
TIdZQg0vtD9yHVRfzmuPo753319s7TrhjKBX3xxvCkBbu/CNRr+059/cRoQvR7ef
thwMIFKL+zx44z9kT36uND13RcOvb7BQEAfpFTY3ddl0AOujJZd0kWpOeSwRrxPP
HB1K/uG3BSGbaHyVbKsXIker45hGMMb43lvj4MVEeK1FMPLIskqOxa9NTDkbNc23
ffidVn9dAgMBAAECggEAI9GOKtro+MPxBe+20trYhMuKmeyCX7bfFsjgAcT74YyQ
nhaCF0LNhlCSyCSKgvyJRoFGcUT9eVpsS+ORTdeW/dcPMIjQLpBzoXUY8qmZfETi
PtCpqvEQQ5qgyzbcs5khYlXXypoeTjRxdl7UqAUynD43pvwRMyUnt87bgLqc7GXH
ABUCRXi3FenshHTV57hp1Hh95xwgSUCe5gJPlQZo6d2swheQ77dUvtJoAGqvHM9N
/pxbNCdZoIb5/PjVAOsd52R7uChxnd40qgya+uA7zAEbhGviOWyRRFHEkQO/RopN
/vg0ZbHid28+ZX6ijsHKV7GKbKGv76ka0uCccITowQKBgQDpGfxhMk5Mu371d3UD
CqGbhfrmwSYOwBxys7fTxytFYx34nR1Q4j9RF4uV/nTLerBuMmVNw602PxYyJHP8
0afMsT3xwP/a0gjl8jwt0KbAFy+gjpoHxer1DNFcF7DQg3UKlMVnn8QDsvezfds3
N6Umf9Dl5G0o19yd5CeNIcpPIQKBgQDLiD4scSoWvVszzaTZocNlWGGxZWFy2Ye7
BV+yTGpJDHn6H8ajcxd5lo3/7ZSUQPIsVyjWDAKjGA/kL8r7Aq6YNzJiMlcTInK4
yWBA0Q3p8FXkiX0lNEL+PuI3ycAot1U0sLE8LZeAy+N3X7F9Fk3dKMcg0V51/LDN
CRl/hNuUvQKBgQCQ/EvBPOP80CZAkYOjV6p7LJOJkZuVUyKeqW/udpRQfTz4FOlW
FNNjIez9Z57HrVEtyYS/ILWM5yJsH8ZQ+yqOo7Ouueep+Df2pnuN15jQI9vI1smx
igYBU26pBEdC+nEDGtPKB1KJJnjxGJgQOTkswBVz2GeZHuKnBnEfVGQcYQKBgQCa
z7nC4hzKiSNrBtuCMlnGp3A/l8aErlNgfNjqbNdXUucgyrSztKJBeLPv3A1squ3J
rk5AaYhD99R2k6fIP6T/4NQw/utegZBTX9EX3CvCKm2a1L1c5CCk9L3rA0lnbvOf
jVpyVJdtfyg4r4/4flOhihfUrYw1IIx2mJpNdYfz3QKBgEhh1kKJh7kUU4z5sHYa
9qXScx4F//4gtE6J3q9c6xgrxANXskmHOIFY+ajMZs4kGrPG6UROUHQnBcHGJlWK
tHc5rn0ius/pdKK/WIyNPm8vN3FtxZAW7PNa5Sewv6L5Jt63TDTK1RD/9mXZHkmH
vbM9JUcwJ+eE0bDv+9fWIZkM
-----END PRIVATE KEY-----`;

const cert = new X509Certificate(certPem);
const matchingPrivate = createPrivateKey(matchingPrivatePem);
const otherPrivate = createPrivateKey(otherPrivatePem);
const publicKey = createPublicKey(matchingPrivate);
const secretKey = createSecretKey(Buffer.alloc(16));

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(
      `${label}: err`,
      err.name,
      err.code ?? "",
      String(err.message).includes("KeyObject"),
    );
  }
}

console.log("typeof checkPrivateKey:", typeof cert["checkPrivateKey"]);
report("matching private", () => cert["checkPrivateKey"](matchingPrivate));
report("other private", () => cert["checkPrivateKey"](otherPrivate));
report("public key", () => cert["checkPrivateKey"](publicKey));
report("secret key", () => cert["checkPrivateKey"](secretKey));
report("missing", () => cert["checkPrivateKey"]());
report("string", () => cert["checkPrivateKey"](matchingPrivatePem as any));
