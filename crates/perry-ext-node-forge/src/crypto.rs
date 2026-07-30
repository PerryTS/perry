//! Pure-Rust PKI core for the node-forge wrapper.
//!
//! No FFI here — everything in this module is plain Rust that operates
//! on owned data (`CertSpec`, PEM strings). That keeps the RSA keygen /
//! X.509 build-and-sign / PEM round-trip logic unit-testable and
//! openssl-verifiable without linking the perry runtime. `lib.rs` is a
//! thin FFI shell that marshals JS values into these types.
//!
//! Fidelity target: byte-shapes that real TLS clients (and `openssl
//! verify`) accept, matching what `node-forge` emits for Socket
//! Firewall's TLS-MITM CA:
//!   - private keys → PKCS#1 `-----BEGIN RSA PRIVATE KEY-----`
//!   - public keys  → SPKI `-----BEGIN PUBLIC KEY-----`
//!   - certificates → `-----BEGIN CERTIFICATE-----`, signed
//!     `sha256WithRSAEncryption`.

use std::str::FromStr;

use const_oid::ObjectIdentifier;
use der::asn1::{Ia5String, OctetString, SetOfVec, Utf8StringRef};
use der::flagset::FlagSet;
use der::{Any, Decode, DecodePem, Encode, EncodePem};
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePublicKey, EncodePublicKey};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use x509_cert::attr::AttributeTypeAndValue;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::ext::pkix::constraints::BasicConstraints;
use x509_cert::ext::pkix::name::{GeneralName, GeneralNames};
use x509_cert::ext::pkix::{
    ExtendedKeyUsage, KeyUsage, KeyUsages, SubjectAltName, SubjectKeyIdentifier,
};
use x509_cert::ext::AsExtension;
use x509_cert::name::{Name, RdnSequence, RelativeDistinguishedName};
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::{Time, Validity};

// ── OIDs ────────────────────────────────────────────────────────────
const OID_CN: &str = "2.5.4.3"; // commonName
const OID_O: &str = "2.5.4.10"; // organizationName
const OID_OU: &str = "2.5.4.11"; // organizationalUnitName
const OID_C: &str = "2.5.4.6"; // countryName
const OID_ST: &str = "2.5.4.8"; // stateOrProvinceName
const OID_L: &str = "2.5.4.7"; // localityName

const OID_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
const OID_CLIENT_AUTH: &str = "1.3.6.1.5.5.7.3.2";

/// A distinguished-name attribute as forge passes it: `{ name?, shortName?, value }`.
#[derive(Debug, Clone)]
pub struct Attr {
    /// forge `name` (e.g. `commonName`) or `shortName` (e.g. `CN`).
    pub key: String,
    pub value: String,
    /// Original ASN.1 string type when the attribute came from a certificate.
    ///
    /// node-forge preserves this information internally. Carrying it through
    /// the forge-shaped JS object keeps a parsed CA subject byte-identical when
    /// it is reused as a leaf issuer.
    pub value_tag: Option<DnValueTag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnValueTag {
    Utf8,
    Printable,
    Ia5,
}

impl DnValueTag {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Printable => "printable",
            Self::Ia5 => "ia5",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "utf8" => Some(Self::Utf8),
            "printable" => Some(Self::Printable),
            "ia5" => Some(Self::Ia5),
            _ => None,
        }
    }
}

/// The forge extension descriptors this wrapper supports.
#[derive(Debug, Clone, Default)]
pub struct ExtSet {
    pub basic_constraints: Option<BasicConstraintsSpec>,
    pub key_usage: Option<KeyUsageSpec>,
    pub ext_key_usage: Option<ExtKeyUsageSpec>,
    pub subject_alt_names: Vec<String>,
    pub subject_key_identifier: bool,
}

#[derive(Debug, Clone)]
pub struct BasicConstraintsSpec {
    pub ca: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Default)]
pub struct KeyUsageSpec {
    pub digital_signature: bool,
    pub key_encipherment: bool,
    pub key_cert_sign: bool,
    pub crl_sign: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ExtKeyUsageSpec {
    pub server_auth: bool,
    pub client_auth: bool,
}

/// Everything needed to build + sign one certificate.
#[derive(Debug, Clone)]
pub struct CertSpec {
    /// SPKI PEM of the certificate's OWN public key.
    pub public_key_pem: String,
    /// Serial number as a hex string (forge convention: `"01"`, `"02"`).
    pub serial_hex: String,
    pub not_before_unix: i64,
    pub not_after_unix: i64,
    pub subject: Vec<Attr>,
    pub issuer: Vec<Attr>,
    pub extensions: ExtSet,
}

// ── keygen + key PEM round-trips ────────────────────────────────────

/// Generate an RSA keypair, returning `(privatePkcs1Pem, publicSpkiPem)`.
pub fn generate_key_pair(bits: usize) -> Result<(String, String), String> {
    let bits = if bits == 0 { 2048 } else { bits };
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, bits).map_err(|e| e.to_string())?;
    let pub_key = RsaPublicKey::from(&priv_key);
    let priv_pem = priv_key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .map_err(|e| e.to_string())?
        .to_string();
    let pub_pem = pub_key
        .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| e.to_string())?;
    Ok((priv_pem, pub_pem))
}

/// Parse a private key from PKCS#1 or PKCS#8 PEM and re-emit as
/// canonical PKCS#1 PEM (forge's `privateKeyToPem` shape).
pub fn normalize_private_key_pem(pem: &str) -> Result<String, String> {
    let key = load_private_key(pem)?;
    Ok(key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .map_err(|e| e.to_string())?
        .to_string())
}

/// Load an `RsaPrivateKey` from either PKCS#1 or PKCS#8 PEM.
pub fn load_private_key(pem: &str) -> Result<RsaPrivateKey, String> {
    if pem.contains("BEGIN RSA PRIVATE KEY") {
        RsaPrivateKey::from_pkcs1_pem(pem).map_err(|e| e.to_string())
    } else {
        use rsa::pkcs8::DecodePrivateKey;
        RsaPrivateKey::from_pkcs8_pem(pem).map_err(|e| e.to_string())
    }
}

fn load_public_key(pem: &str) -> Result<RsaPublicKey, String> {
    if pem.contains("BEGIN RSA PUBLIC KEY") {
        use rsa::pkcs1::DecodeRsaPublicKey;
        RsaPublicKey::from_pkcs1_pem(pem).map_err(|e| e.to_string())
    } else {
        RsaPublicKey::from_public_key_pem(pem).map_err(|e| e.to_string())
    }
}

// ── DN <-> attrs ────────────────────────────────────────────────────

fn oid_for(key: &str) -> Result<ObjectIdentifier, String> {
    let oid_str = match key {
        "commonName" | "CN" => OID_CN,
        "organizationName" | "O" => OID_O,
        "organizationalUnitName" | "OU" => OID_OU,
        "countryName" | "C" => OID_C,
        "stateOrProvinceName" | "ST" => OID_ST,
        "localityName" | "L" => OID_L,
        // Accept a raw dotted OID too.
        other
            if other
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false) =>
        {
            other
        }
        other => return Err(format!("node-forge: unsupported DN attribute '{other}'")),
    };
    ObjectIdentifier::from_str(oid_str).map_err(|e| e.to_string())
}

fn name_for(oid: &ObjectIdentifier) -> String {
    match oid.to_string().as_str() {
        OID_CN => "commonName",
        OID_O => "organizationName",
        OID_OU => "organizationalUnitName",
        OID_C => "countryName",
        OID_ST => "stateOrProvinceName",
        OID_L => "localityName",
        _ => return oid.to_string(),
    }
    .to_string()
}

/// Build an X.509 `Name` preserving the attribute ORDER exactly. Order
/// preservation is load-bearing: `openssl verify` matches a leaf's
/// issuer DN against the CA's subject DN byte-for-byte, and sfw derives
/// the leaf issuer from `certificateFromPem(ca).subject.attributes` —
/// so `parse_name` (below) must be the exact inverse of this.
fn build_name(attrs: &[Attr]) -> Result<Name, String> {
    let mut rdns = Vec::with_capacity(attrs.len());
    for a in attrs {
        let oid = oid_for(&a.key)?;
        let value = match a.value_tag.unwrap_or(DnValueTag::Utf8) {
            DnValueTag::Utf8 => Any::from(Utf8StringRef::new(&a.value).map_err(|e| e.to_string())?),
            DnValueTag::Printable => {
                Any::from(der::asn1::PrintableStringRef::new(&a.value).map_err(|e| e.to_string())?)
            }
            DnValueTag::Ia5 => {
                Any::from(der::asn1::Ia5StringRef::new(&a.value).map_err(|e| e.to_string())?)
            }
        };
        let atv = AttributeTypeAndValue { oid, value };
        let set = SetOfVec::try_from(vec![atv]).map_err(|e| e.to_string())?;
        rdns.push(RelativeDistinguishedName(set));
    }
    Ok(RdnSequence(rdns))
}

/// Parse a `Name` back into forge-shaped attributes, preserving order.
pub fn parse_name(name: &Name) -> Vec<Attr> {
    let mut out = Vec::new();
    for rdn in name.0.iter() {
        for atv in rdn.0.iter() {
            let decoded = atv
                .value
                .decode_as::<Utf8StringRef<'_>>()
                .map(|s| (s.as_str().to_string(), DnValueTag::Utf8))
                .or_else(|_| {
                    atv.value
                        .decode_as::<der::asn1::PrintableStringRef<'_>>()
                        .map(|s| (s.as_str().to_string(), DnValueTag::Printable))
                })
                .or_else(|_| {
                    atv.value
                        .decode_as::<der::asn1::Ia5StringRef<'_>>()
                        .map(|s| (s.as_str().to_string(), DnValueTag::Ia5))
                });
            if let Ok((value, value_tag)) = decoded {
                out.push(Attr {
                    key: name_for(&atv.oid),
                    value,
                    value_tag: Some(value_tag),
                });
            }
        }
    }
    out
}

// ── serial + validity ──────────────────────────────────────────────

fn serial_from_hex(hex: &str) -> Result<SerialNumber, String> {
    let trimmed = hex.trim().trim_start_matches("0x");
    let padded = if trimmed.len() % 2 == 1 {
        format!("0{trimmed}")
    } else {
        trimmed.to_string()
    };
    let mut bytes = Vec::with_capacity(padded.len() / 2);
    let chars: Vec<char> = padded.chars().collect();
    for pair in chars.chunks(2) {
        let byte = u8::from_str_radix(&pair.iter().collect::<String>(), 16)
            .map_err(|_| format!("node-forge: invalid serialNumber hex '{hex}'"))?;
        bytes.push(byte);
    }
    if bytes.is_empty() {
        bytes.push(1);
    }
    // A leading high bit would make the INTEGER negative; DER serials
    // are positive, so prepend a zero byte like forge/openssl do.
    if bytes[0] & 0x80 != 0 {
        bytes.insert(0, 0x00);
    }
    SerialNumber::new(&bytes).map_err(|e| e.to_string())
}

fn time_from_unix(secs: i64) -> Result<Time, String> {
    let dur = std::time::Duration::from_secs(secs.max(0) as u64);
    let sys = std::time::UNIX_EPOCH + dur;
    Time::try_from(sys).map_err(|e| e.to_string())
}

// ── extensions ──────────────────────────────────────────────────────

fn key_usage_ext(spec: &KeyUsageSpec) -> KeyUsage {
    let mut flags: FlagSet<KeyUsages> = FlagSet::default();
    if spec.digital_signature {
        flags |= KeyUsages::DigitalSignature;
    }
    if spec.key_encipherment {
        flags |= KeyUsages::KeyEncipherment;
    }
    if spec.key_cert_sign {
        flags |= KeyUsages::KeyCertSign;
    }
    if spec.crl_sign {
        flags |= KeyUsages::CRLSign;
    }
    KeyUsage(flags)
}

fn ext_key_usage_ext(spec: &ExtKeyUsageSpec) -> Result<ExtendedKeyUsage, String> {
    let mut oids = Vec::new();
    if spec.server_auth {
        oids.push(ObjectIdentifier::from_str(OID_SERVER_AUTH).map_err(|e| e.to_string())?);
    }
    if spec.client_auth {
        oids.push(ObjectIdentifier::from_str(OID_CLIENT_AUTH).map_err(|e| e.to_string())?);
    }
    Ok(ExtendedKeyUsage(oids))
}

fn subject_alt_name_ext(hosts: &[String]) -> Result<SubjectAltName, String> {
    let mut names: GeneralNames = Vec::new();
    for h in hosts {
        let ia5 = Ia5String::new(h).map_err(|e| e.to_string())?;
        names.push(GeneralName::DnsName(ia5));
    }
    Ok(SubjectAltName(names))
}

// ── build + sign ────────────────────────────────────────────────────

/// Preserve node-forge's caller-supplied `critical` bit instead of accepting
/// x509-cert's policy default for an extension.
struct ExtensionWithCritical<'a, E> {
    value: &'a E,
    critical: bool,
}

impl<E: const_oid::AssociatedOid> const_oid::AssociatedOid for ExtensionWithCritical<'_, E> {
    const OID: ObjectIdentifier = E::OID;
}

impl<E: Encode> Encode for ExtensionWithCritical<'_, E> {
    fn encoded_len(&self) -> der::Result<der::Length> {
        self.value.encoded_len()
    }

    fn encode(&self, writer: &mut impl der::Writer) -> der::Result<()> {
        self.value.encode(writer)
    }
}

impl<E: const_oid::AssociatedOid + Encode> AsExtension for ExtensionWithCritical<'_, E> {
    fn critical(&self, _subject: &Name, _extensions: &[x509_cert::ext::Extension]) -> bool {
        self.critical
    }
}

/// Build and sign a certificate. `signer_private_key_pem` is the
/// ISSUER's private key (for a self-signed CA it is the same key whose
/// public half is in `spec.public_key_pem`).
pub fn build_and_sign(spec: &CertSpec, signer_private_key_pem: &str) -> Result<String, String> {
    let signer_key = load_private_key(signer_private_key_pem)?;
    let signing_key = SigningKey::<Sha256>::new(signer_key);

    let subject = build_name(&spec.subject)?;
    let issuer = build_name(&spec.issuer)?;
    let serial = serial_from_hex(&spec.serial_hex)?;
    let validity = Validity {
        not_before: time_from_unix(spec.not_before_unix)?,
        not_after: time_from_unix(spec.not_after_unix)?,
    };

    let cert_pub = load_public_key(&spec.public_key_pem)?;
    let spki_der = cert_pub
        .to_public_key_der()
        .map_err(|e| e.to_string())?
        .into_vec();
    let spki = SubjectPublicKeyInfoOwned::from_der(&spki_der).map_err(|e| e.to_string())?;

    // Profile::Manual gives us exact control: it injects no extensions
    // of its own, so the cert carries precisely what sfw requested.
    let profile = Profile::Manual {
        issuer: Some(issuer),
    };

    let mut builder =
        CertificateBuilder::new(profile, serial, validity, subject, spki, &signing_key)
            .map_err(|e| e.to_string())?;

    let exts = &spec.extensions;
    if let Some(bc) = &exts.basic_constraints {
        let extension = BasicConstraints {
            ca: bc.ca,
            path_len_constraint: None,
        };
        builder
            .add_extension(&ExtensionWithCritical {
                value: &extension,
                critical: bc.critical,
            })
            .map_err(|e| e.to_string())?;
    }
    if let Some(ku) = &exts.key_usage {
        let extension = key_usage_ext(ku);
        builder
            .add_extension(&ExtensionWithCritical {
                value: &extension,
                critical: ku.critical,
            })
            .map_err(|e| e.to_string())?;
    }
    if let Some(eku) = &exts.ext_key_usage {
        builder
            .add_extension(&ext_key_usage_ext(eku)?)
            .map_err(|e| e.to_string())?;
    }
    if !exts.subject_alt_names.is_empty() {
        builder
            .add_extension(&subject_alt_name_ext(&exts.subject_alt_names)?)
            .map_err(|e| e.to_string())?;
    }
    if exts.subject_key_identifier {
        let ski = compute_ski(&spki_der)?;
        builder.add_extension(&ski).map_err(|e| e.to_string())?;
    }

    let cert = builder
        .build::<rsa::pkcs1v15::Signature>()
        .map_err(|e| e.to_string())?;
    cert.to_pem(der::pem::LineEnding::LF)
        .map_err(|e| e.to_string())
}

/// SubjectKeyIdentifier = SHA-1 of the DER-encoded subjectPublicKey BIT
/// STRING contents (RFC 5280 method 1). We hash the whole SPKI DER's
/// public-key bytes; openssl accepts any 20-byte SKI here (it is not
/// checked by `verify`).
fn compute_ski(spki_der: &[u8]) -> Result<SubjectKeyIdentifier, String> {
    let spki = SubjectPublicKeyInfoOwned::from_der(spki_der).map_err(|e| e.to_string())?;
    let key_bytes = spki
        .subject_public_key
        .as_bytes()
        .ok_or("node-forge: SPKI bit-string not byte-aligned")?;
    use sha2::Digest;
    // SHA-256 truncated to 20 bytes — deterministic, display-only.
    let digest = Sha256::digest(key_bytes);
    let octet = OctetString::new(&digest[..20]).map_err(|e| e.to_string())?;
    Ok(SubjectKeyIdentifier(octet))
}

/// Parse the subject attributes out of a certificate PEM (for
/// `certificateFromPem(...).subject.attributes`).
pub fn cert_subject_attrs(pem: &str) -> Result<Vec<Attr>, String> {
    let cert = x509_cert::Certificate::from_pem(pem).map_err(|e| e.to_string())?;
    Ok(parse_name(&cert.tbs_certificate.subject))
}

#[cfg(test)]
mod tests {
    use super::*;
    use const_oid::AssociatedOid;
    use der::Encode;
    use rsa::pkcs1::EncodeRsaPrivateKey;

    fn ca_spec(pub_pem: &str) -> CertSpec {
        CertSpec {
            public_key_pem: pub_pem.to_string(),
            serial_hex: "01".to_string(),
            not_before_unix: 1_700_000_000,
            not_after_unix: 1_800_000_000,
            subject: vec![
                Attr {
                    key: "commonName".into(),
                    value: "Socket Security CA".into(),
                    value_tag: None,
                },
                Attr {
                    key: "organizationName".into(),
                    value: "Socket Security".into(),
                    value_tag: None,
                },
            ],
            issuer: vec![
                Attr {
                    key: "commonName".into(),
                    value: "Socket Security CA".into(),
                    value_tag: None,
                },
                Attr {
                    key: "organizationName".into(),
                    value: "Socket Security".into(),
                    value_tag: None,
                },
            ],
            extensions: ExtSet {
                basic_constraints: Some(BasicConstraintsSpec {
                    ca: true,
                    critical: true,
                }),
                key_usage: Some(KeyUsageSpec {
                    key_cert_sign: true,
                    critical: true,
                    ..Default::default()
                }),
                subject_key_identifier: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn keygen_pem_shapes() {
        let (priv_pem, pub_pem) = generate_key_pair(2048).unwrap();
        assert!(priv_pem.contains("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(pub_pem.contains("-----BEGIN PUBLIC KEY-----"));
    }

    #[test]
    fn private_key_pem_round_trip() {
        let (priv_pem, _) = generate_key_pair(2048).unwrap();
        let norm = normalize_private_key_pem(&priv_pem).unwrap();
        // Re-loading the normalized PEM yields the same modulus.
        let a = load_private_key(&priv_pem).unwrap();
        let b = load_private_key(&norm).unwrap();
        assert_eq!(
            a.to_pkcs1_der().unwrap().as_bytes(),
            b.to_pkcs1_der().unwrap().as_bytes()
        );
    }

    #[test]
    fn self_signed_ca_builds_and_parses() {
        let (priv_pem, pub_pem) = generate_key_pair(2048).unwrap();
        let ca_pem = build_and_sign(&ca_spec(&pub_pem), &priv_pem).unwrap();
        assert!(ca_pem.contains("-----BEGIN CERTIFICATE-----"));
        let attrs = cert_subject_attrs(&ca_pem).unwrap();
        // Order-preserving round-trip: CN first, O second.
        assert_eq!(attrs[0].key, "commonName");
        assert_eq!(attrs[0].value, "Socket Security CA");
        assert_eq!(attrs[1].key, "organizationName");
    }

    #[test]
    fn issuer_dn_matches_ca_subject_dn() {
        // The make-or-break for `openssl verify`: leaf.issuer built from
        // parsed CA subject attrs must equal ca.subject byte-for-byte.
        let (priv_pem, pub_pem) = generate_key_pair(2048).unwrap();
        let ca_pem = build_and_sign(&ca_spec(&pub_pem), &priv_pem).unwrap();
        let ca = x509_cert::Certificate::from_pem(&ca_pem).unwrap();
        let parsed_attrs = parse_name(&ca.tbs_certificate.subject);
        let rebuilt = build_name(&parsed_attrs).unwrap();
        assert_eq!(
            rebuilt.to_der().unwrap(),
            ca.tbs_certificate.subject.to_der().unwrap(),
            "rebuilt issuer DN must match CA subject DN exactly"
        );
    }

    #[test]
    fn dn_round_trip_preserves_string_tags_and_unknown_oids() {
        let original = build_name(&[
            Attr {
                key: "countryName".into(),
                value: "US".into(),
                value_tag: Some(DnValueTag::Printable),
            },
            Attr {
                key: "1.2.840.113549.1.9.1".into(),
                value: "ca@example.com".into(),
                value_tag: Some(DnValueTag::Ia5),
            },
        ])
        .unwrap();

        let parsed = parse_name(&original);
        assert_eq!(parsed[0].value_tag, Some(DnValueTag::Printable));
        assert_eq!(parsed[1].key, "1.2.840.113549.1.9.1");
        assert_eq!(parsed[1].value_tag, Some(DnValueTag::Ia5));
        assert_eq!(
            build_name(&parsed).unwrap().to_der().unwrap(),
            original.to_der().unwrap()
        );
    }

    #[test]
    fn requested_extension_critical_flags_are_preserved() {
        let (priv_pem, pub_pem) = generate_key_pair(2048).unwrap();
        let mut spec = ca_spec(&pub_pem);
        spec.extensions.basic_constraints.as_mut().unwrap().critical = false;
        spec.extensions.key_usage.as_mut().unwrap().critical = false;
        let pem = build_and_sign(&spec, &priv_pem).unwrap();
        let cert = x509_cert::Certificate::from_pem(&pem).unwrap();
        let extensions = cert.tbs_certificate.extensions.as_ref().unwrap();
        let basic_constraints = extensions
            .iter()
            .find(|ext| ext.extn_id == BasicConstraints::OID)
            .unwrap();
        let key_usage = extensions
            .iter()
            .find(|ext| ext.extn_id == KeyUsage::OID)
            .unwrap();
        assert!(!basic_constraints.critical);
        assert!(!key_usage.critical);
    }
}
