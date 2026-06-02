use std::path::Path;
use std::sync::OnceLock;

const CA_BUNDLE_PATHS: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

const FALLBACK_ROOT_CERTS: &[&str] = &[
    r#"-----BEGIN CERTIFICATE-----
MIIFgzCCA2ugAwIBAgIPXZONMGc2yAYdGsdUhGkHMA0GCSqGSIb3DQEBCwUAMDsx
CzAJBgNVBAYTAkVTMREwDwYDVQQKDAhGTk1ULVJDTTEZMBcGA1UECwwQQUMgUkFJ
WiBGTk1ULVJDTTCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBALpxgHpM
hm5/yBNtwMZ9HACXjywMI7sQmkCpGreHiPibVmr75nuOi5KOpyVdWRHbNi63URcfq
QgfBBckWKo3Shjf5TnUV/3XwSyRAZHiItQDwFj8d0fsjz50Q7qsNI1NOHZnjrDIb
zAzWHFctPVrbtQBULgTfmxKo0nRIBnuvMApGGWn3v7v3QqQIecaZ5JCEJhfTzC8P
hxFtBDXaEAUwED653cXeuYLj2VbPNmaUtu1vZ5Gzz3rkQUCwJaydkxNEJY7kvqcf
w+Z374jNUUeAlz+taibmSXaXvMiwzn15Cou08YfxGyqxRxqAQVKL9LFwag0Jl1mp
dICIfkYtwb1TplvqKtMUejPUBjFd8g5CSxJkjKZqLsXF3mwWsXmo8RZZUc1g16p6
DULmbvkzSDGm0oGObVo/CK67lWMK07q87Hj/LaZmtVC+nFNCM+HHmpxffnTtOmlc
YF7wk5HlqX2doWjKI/pgG6BU6VtX7hI+cL5NqYuSf+4lsKMB7ObiFj86xsc3i1w
4peSMKGJ47xVqCfWS+2QrYv6YyVZLag13cqXM7zlzced0ezvXg5KkAYmY6252TUt
B7p2ZSysV4999AeU14ECll2jB0nVetBX+RvnU0Z1qrB5QstocQjpYL05ac70r8NW
QMetUqIJ5G+GR4of6ygnXYMgrwTJbFaai0b1AgMBAAGjgYMwgYAwDwYDVR0TAQH/
BAUwAwEB/zAOBgNVHQ8BAf8EBAMCAQYwHQYDVR0OBBYEFPd9xf3E6Jobd2Sn9R2g
zL+HYJptMD4GA1UdIAQ3MDUwMwYEVR0gADArMCkGCCsGAQUFBwIBFh1odHRwOi8v
d3d3LmNlcnQuZm5tdC5lcy9kcGNzLzANBgkqhkiG9w0BAQsFAAOCAgEAB5BK3/Mj
TvDDnFFlm5wioooMhfNzKWtN/gHiqQxjAb8EZ6WdmF/9ARP67Jpi6Yb+tmLSbkyU
+8B1RXxlDPiyN8+sD8+Nb/kZ94/sHvJwnvDKuO+3/3Y3dlv2bojzr2IyIpMNOmq
OFGYMLVN0V2Ue1bLdI4E7pWYjJ2cJj+F3qkPNZVEI7VFY/uY5+ctHhKQV8Xa7pO6
kO8Rf77IzlhEYt8llvhjho6Tc+hj507wTmzl6NLrTQfv6MooqtyuGC2mDOL7Nii4
LcK2NJpLuHvUBKwrZ1pebbuCoGRw6IYsMHkCtA+fdZn71uSANA+iW+YJF1DngoAB
d15jmfZ5nc8OaKveri6E6FO80vFIOiZiaBECEHX5FaZNXzuvO+FB8TxxuBEOb+dY
7Ixjp6o7RTUaN8Tvkasq6+yO3m/qZASlaWFot4/nUbQ4mrcFuNLwy+AwF+mWj2zs
3gyLp1txyM/1d8iC9djwj2ij3+RvrWWTV3F9yfiD8zYm1kGdNYno/Tq0dwzn+ev
QoFt9B9kiABdcPUXmsEKvU7ANm5mqwujGSQkBqvjrTcuFqN1W8rB2Vt2lh8kORdO
ag0wokRqEIr9baRRmW1FMdW4R58MD3R++Lj8UGrp1MYp3/RgT408m2ECVAdf4Wqs
lKYIYvuu8wd+RU4riEmViAqhOLUTpPSPaLtrM=
-----END CERTIFICATE-----"#,
    r#"-----BEGIN CERTIFICATE-----
MIICbjCCAfOgAwIBAgIQYvYybOXE42hcG2LdnC6dlTAKBggqhkjOPQQDAzB4MQsw
CQYDVQQGEwJFUzERMA8GA1UECgwIRk5NVC1SQ00xDjAMBgNVBAsMBUNlcmVzMRgw
FgYDVQRhDA9WQVRFUy1RMjgyNjAwNEoxLDAqBgNVBAMMI0FDIFJBSVogRk5NVC1S
Q00gU0VSVklET1JFUyBTRUdVUk9TMB4XDTE4MTIyMDA5MzczM1oXDTQzMTIyMDA5
MzczM1oweDELMAkGA1UEBhMCRVMxETAPBgNVBAoMCEZOTVQtUkNNMQ4wDAYDVQQL
DAVDZXJlczEYMBYGA1UEYQwPVkFURVMtUTI4MjYwMDRKMSwwKgYDVQQDDCNBQyBS
QUlaIEZOTVQtUkNNIFNFUlZJRE9SRVMgU0VHVVJPUzB2MBAGByqGSM49AgEGBSuB
BAAiA2IABPa6V1PIyqvfNkpSIeSX0oNnnvBlUdBeh8dHsVnyV0ebAAKTRBdp20LH
sbI6GA60XYyzZl2hNPk2LEnb80b8s0RpRBNm/dfF/a82Tc4DTQdxz69qBdKiQ1oK
Um8BA06Oi6NCMEAwDwYDVR0TAQH/BAUwAwEB/zAOBgNVHQ8BAf8EBAMCAQYwHQYD
VR0OBBYEFAG5L++/EYZg8k/QQW6rcx/n0m5JMAoGCCqGSM49BAMDA2kAMGYCMQCu
SuMrQMN0EfKVrRYj3k4MGuZdpSRea0R7/DjiT8ucRRcRTBQnJlU5dUoDzBOQn5IC
MQD6SmxgiHPz7riYYqnOK8LZiqZwMR2vsJRM60/G49HzYqc8/5MuB1xJAWdpEgJy
v+c=
-----END CERTIFICATE-----"#,
];

static BUNDLED_CERTS: OnceLock<Vec<String>> = OnceLock::new();
static SYSTEM_CERTS: OnceLock<Vec<String>> = OnceLock::new();
static EXTRA_CERTS: OnceLock<Vec<String>> = OnceLock::new();

pub(crate) fn bundled_certificates() -> &'static [String] {
    BUNDLED_CERTS
        .get_or_init(|| multi_cert_or_fallback(load_first_existing_bundle()))
        .as_slice()
}

pub(crate) fn system_certificates() -> &'static [String] {
    SYSTEM_CERTS
        .get_or_init(|| multi_cert_or_fallback(load_first_existing_bundle()))
        .as_slice()
}

pub(crate) fn extra_certificates() -> &'static [String] {
    EXTRA_CERTS
        .get_or_init(|| {
            std::env::var_os("NODE_EXTRA_CA_CERTS")
                .and_then(|path| load_pem_file(Path::new(&path)))
                .unwrap_or_default()
        })
        .as_slice()
}

fn load_first_existing_bundle() -> Vec<String> {
    std::env::var_os("SSL_CERT_FILE")
        .and_then(|path| load_pem_file(Path::new(&path)))
        .filter(|certs| certs.len() > 1)
        .or_else(|| {
            CA_BUNDLE_PATHS
                .iter()
                .find_map(|path| load_pem_file(Path::new(path)).filter(|certs| certs.len() > 1))
        })
        .unwrap_or_default()
}

fn load_pem_file(path: &Path) -> Option<Vec<String>> {
    let contents = std::fs::read_to_string(path).ok()?;
    let certs = split_pem_certificates(&contents);
    if certs.is_empty() {
        None
    } else {
        Some(certs)
    }
}

fn multi_cert_or_fallback(certs: Vec<String>) -> Vec<String> {
    if certs.len() > 1 {
        return certs;
    }
    FALLBACK_ROOT_CERTS
        .iter()
        .map(|cert| format!("{}\n", cert.trim_end()))
        .collect()
}

pub(crate) fn split_pem_certificates(contents: &str) -> Vec<String> {
    let mut certs = Vec::new();
    let mut current = String::new();
    let mut in_cert = false;

    for line in contents.lines() {
        if line.contains("-----BEGIN CERTIFICATE-----") {
            current.clear();
            in_cert = true;
        }
        if in_cert {
            current.push_str(line.trim_end());
            current.push('\n');
        }
        if line.contains("-----END CERTIFICATE-----") && in_cert {
            certs.push(std::mem::take(&mut current));
            in_cert = false;
        }
    }

    certs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_multiple_pem_blocks() {
        let joined = format!("{}\n{}\n", FALLBACK_ROOT_CERTS[0], FALLBACK_ROOT_CERTS[1]);
        let certs = split_pem_certificates(&joined);
        assert_eq!(certs.len(), 2);
        assert!(certs[0].starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(certs[1].ends_with("-----END CERTIFICATE-----\n"));
    }

    #[test]
    fn fallback_is_real_multi_cert_inventory() {
        let certs = multi_cert_or_fallback(Vec::new());
        assert!(certs.len() > 1);
        assert!(certs.iter().all(|cert| {
            cert.starts_with("-----BEGIN CERTIFICATE-----")
                && cert.contains("-----END CERTIFICATE-----")
                && !cert.contains("Perry Runtime Root CA")
                && !cert.contains("Perry Test")
        }));
    }
}
