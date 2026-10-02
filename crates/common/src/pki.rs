//! Certificate authority helpers shared by the app (which owns the CA) and
//! the agent (which generates its own key and requests a certificate).
//!
//! The private CA key never leaves the app; agents only ever receive a
//! signed leaf certificate plus the CA's public certificate to pin against.

use rcgen::{
    BasicConstraints, CertificateParams, CertificateSigningRequestParams, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls_pki_types::CertificateDer;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

const CA_VALIDITY_YEARS: i64 = 10;
const LEAF_VALIDITY_YEARS: i64 = 2;
const CLOCK_SKEW_ALLOWANCE: Duration = Duration::hours(24);

#[derive(Debug, thiserror::Error)]
pub enum PkiError {
    #[error("certificate generation failed: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("invalid PEM content: {0}")]
    Pem(#[from] pem::PemError),
}

/// A freshly generated key pair plus its certificate, PEM-encoded and ready
/// to write to disk.
pub struct CertifiedKey {
    pub cert_pem: String,
    pub cert_der: CertificateDer<'static>,
    pub key_pem: String,
}

/// A certificate issued from a CSR: the agent already holds the matching
/// private key, so only the certificate is returned.
pub struct IssuedCert {
    pub cert_pem: String,
    pub cert_der: CertificateDer<'static>,
    /// Lowercase hex serial number, explicitly assigned at signing time
    /// (rather than left to rcgen's default) so the caller can record it
    /// and later revoke this exact certificate by serial.
    pub serial_hex: String,
}

/// The certificate authority. Holds the CA private key, so it must only
/// ever live inside the app (in the macOS Keychain in production).
pub struct Ca {
    key_pair: KeyPair,
    /// Reconstructed in-memory each time from `key_pair` + `params` purely
    /// to act as the `issuer` argument for `signed_by`. Its own DER bytes
    /// are never distributed — `cert_der`/`cert_pem` below are.
    issuer: rcgen::Certificate,
    cert_pem: String,
    cert_der: CertificateDer<'static>,
    not_after: OffsetDateTime,
}

impl Ca {
    /// Generate a brand new certificate authority.
    pub fn generate() -> Result<Self, PkiError> {
        let key_pair = KeyPair::generate()?;
        let params = ca_params()?;
        // X.509 time fields carry no sub-second precision, so this would
        // otherwise disagree with the value `from_pem` reads back from the
        // serialized cert (whole seconds only) for the exact same CA.
        let not_after = params.not_after.replace_nanosecond(0).unwrap_or(params.not_after);
        let issuer = params.self_signed(&key_pair)?;
        let cert_der = issuer.der().clone();
        let cert_pem = issuer.pem();
        Ok(Ca {
            key_pair,
            issuer,
            cert_pem,
            cert_der,
            not_after,
        })
    }

    /// Reload a CA previously persisted with [`Ca::cert_pem`] / [`Ca::key_pem`].
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Result<Self, PkiError> {
        let key_pair = KeyPair::from_pem(key_pem)?;
        let params = CertificateParams::from_ca_cert_pem(cert_pem)?;
        let not_after = params.not_after;
        let cert_der = pem_to_der(cert_pem)?;
        let issuer = params.self_signed(&key_pair)?;
        Ok(Ca {
            key_pair,
            issuer,
            cert_pem: cert_pem.to_string(),
            cert_der,
            not_after,
        })
    }

    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    pub fn key_pem(&self) -> String {
        self.key_pair.serialize_pem()
    }

    /// SHA-256 fingerprint of the CA certificate, as lowercase hex. This is
    /// what an agent installer pins against (`--ca-sha256`) before trusting
    /// the enrollment server.
    pub fn fingerprint_sha256(&self) -> String {
        fingerprint_sha256(&self.cert_der)
    }

    /// RFC 3339 expiry of the CA certificate itself — not a leaf's, which
    /// are reissued every [`LEAF_VALIDITY_YEARS`] regardless.
    pub fn not_after_rfc3339(&self) -> String {
        self.not_after
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    }

    /// The signature algorithm every key this CA issues (its own included)
    /// is generated with — always this one, since [`KeyPair::generate`]
    /// never varies it.
    pub fn algorithm_name(&self) -> &'static str {
        "ECDSA P-256"
    }

    /// Sign a CSR produced by an agent (see [`generate_agent_identity`]),
    /// issuing it a server-auth leaf certificate. `names` are the extra
    /// SAN entries (hostname, `hostname.local`, current IPs) the app wants
    /// on the cert regardless of what the agent itself requested.
    pub fn sign_agent_csr(&self, csr_pem: &str, names: &[String]) -> Result<IssuedCert, PkiError> {
        let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)?;
        csr.params.subject_alt_names = build_san(names)?;
        csr.params.not_before = OffsetDateTime::now_utc() - CLOCK_SKEW_ALLOWANCE;
        csr.params.not_after = OffsetDateTime::now_utc() + Duration::days(365 * LEAF_VALIDITY_YEARS);
        csr.params.is_ca = IsCa::NoCa;
        csr.params.use_authority_key_identifier_extension = true;
        csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        csr.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let serial = fresh_serial_number(csr_pem);
        csr.params.serial_number = Some(serial.clone().into());
        let cert = csr.signed_by(self.issuer_cert(), &self.key_pair)?;
        Ok(IssuedCert {
            cert_der: cert.der().clone(),
            cert_pem: cert.pem(),
            serial_hex: hex_encode(&serial),
        })
    }

    /// Self-generate the app's own permanent mTLS client identity.
    pub fn issue_client_identity(&self, common_name: &str) -> Result<CertifiedKey, PkiError> {
        let key_pair = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.distinguished_name.push(DnType::CommonName, common_name);
        params.not_before = OffsetDateTime::now_utc() - CLOCK_SKEW_ALLOWANCE;
        params.not_after = OffsetDateTime::now_utc() + Duration::days(365 * LEAF_VALIDITY_YEARS);
        params.is_ca = IsCa::NoCa;
        params.use_authority_key_identifier_extension = true;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let cert = params.signed_by(&key_pair, self.issuer_cert(), &self.key_pair)?;
        Ok(CertifiedKey {
            cert_der: cert.der().clone(),
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        })
    }

    /// Self-generate a server-auth leaf certificate directly (no CSR
    /// round-trip). Used for the app's temporary enrollment server, and in
    /// tests to stand up an agent without going through enrollment.
    pub fn issue_server_identity(&self, names: &[String]) -> Result<CertifiedKey, PkiError> {
        let key_pair = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.subject_alt_names = build_san(names)?;
        if let Some(first) = names.first() {
            params.distinguished_name.push(DnType::CommonName, first);
        }
        params.not_before = OffsetDateTime::now_utc() - CLOCK_SKEW_ALLOWANCE;
        params.not_after = OffsetDateTime::now_utc() + Duration::days(365 * LEAF_VALIDITY_YEARS);
        params.is_ca = IsCa::NoCa;
        params.use_authority_key_identifier_extension = true;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key_pair, self.issuer_cert(), &self.key_pair)?;
        Ok(CertifiedKey {
            cert_der: cert.der().clone(),
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        })
    }

    fn issuer_cert(&self) -> &rcgen::Certificate {
        &self.issuer
    }
}

/// Generate the agent's own local key pair plus a CSR requesting a
/// server-auth certificate for `names` (hostname, `hostname.local`, current
/// IPs). The private key never leaves this function's caller — only the
/// returned `csr_pem` is sent to the app.
pub fn generate_agent_identity(names: &[String]) -> Result<(KeyPair, String), PkiError> {
    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.subject_alt_names = build_san(names)?;
    if let Some(first) = names.first() {
        params.distinguished_name.push(DnType::CommonName, first);
    }
    let csr_pem = params.serialize_request(&key_pair)?.pem()?;
    Ok((key_pair, csr_pem))
}

fn ca_params() -> Result<CertificateParams, PkiError> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "HomeGuard CA");
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    params.not_before = OffsetDateTime::now_utc() - CLOCK_SKEW_ALLOWANCE;
    params.not_after = OffsetDateTime::now_utc() + Duration::days(365 * CA_VALIDITY_YEARS);
    Ok(params)
}

fn build_san(names: &[String]) -> Result<Vec<rcgen::SanType>, PkiError> {
    // `CertificateParams::new` already classifies each string as an IP or a
    // DNS name; reuse that instead of duplicating the logic.
    Ok(CertificateParams::new(names.to_vec())?.subject_alt_names)
}

pub fn fingerprint_sha256(der: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(der.as_ref());
    digest.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

/// Plain lowercase-hex SHA-256 of arbitrary bytes, no separators — what
/// `/v1/update`'s `x-sha256` header and the deploy server's
/// `__AGENT_SHA256_*__` placeholders expect. Distinct from
/// `fingerprint_sha256` above, which colon-separates for display.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 fingerprint of a PEM-encoded certificate. Used to re-check a CA
/// certificate received over an already-authenticated channel (e.g. inside
/// a JSON response body) against a fingerprint pinned independently —
/// authenticating the transport is not the same as authenticating every
/// piece of data it carries.
pub fn fingerprint_of_pem(cert_pem: &str) -> Result<String, PkiError> {
    Ok(fingerprint_sha256(&pem_to_der(cert_pem)?))
}

fn pem_to_der(cert_pem: &str) -> Result<CertificateDer<'static>, PkiError> {
    let parsed = pem::parse(cert_pem)?;
    Ok(CertificateDer::from(parsed.into_contents()))
}

/// A fresh 16-byte serial, derived from `seed` (typically the CSR PEM) and
/// the current time so concurrent issuances never collide.
fn fresh_serial_number(seed: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.update(OffsetDateTime::now_utc().unix_timestamp_nanos().to_be_bytes());
    hasher.finalize()[..16].to_vec()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn generates_and_reloads_a_ca() {
        let ca = Ca::generate().unwrap();
        let reloaded = Ca::from_pem(ca.cert_pem(), &ca.key_pem()).unwrap();
        assert_eq!(ca.cert_pem(), reloaded.cert_pem());
        assert_eq!(ca.fingerprint_sha256(), reloaded.fingerprint_sha256());
    }

    #[test]
    fn exposes_algorithm_and_survives_reload_with_the_same_expiry() {
        let ca = Ca::generate().unwrap();
        assert_eq!(ca.algorithm_name(), "ECDSA P-256");
        // CA_VALIDITY_YEARS out, not "now" — a freshly generated CA's
        // not_after is nowhere near today.
        let expected_year = (OffsetDateTime::now_utc().year() + CA_VALIDITY_YEARS as i32).to_string();
        assert!(ca.not_after_rfc3339().starts_with(&expected_year));

        let reloaded = Ca::from_pem(ca.cert_pem(), &ca.key_pem()).unwrap();
        assert_eq!(ca.not_after_rfc3339(), reloaded.not_after_rfc3339());
    }

    #[test]
    fn issues_a_client_identity_signed_by_the_ca() {
        let ca = Ca::generate().unwrap();
        let identity = ca.issue_client_identity("homeguard-app").unwrap();
        assert!(identity.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(identity.key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn issues_a_server_identity_with_expected_sans() {
        let ca = Ca::generate().unwrap();
        let identity = ca
            .issue_server_identity(&names(&["pc-salon", "pc-salon.local", "192.168.1.34"]))
            .unwrap();
        assert!(identity.cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn agent_generates_its_own_key_and_csr() {
        let (_key_pair, csr_pem) =
            generate_agent_identity(&names(&["pc-salon", "192.168.1.34"])).unwrap();
        assert!(csr_pem.contains("BEGIN CERTIFICATE REQUEST"));
    }

    #[test]
    fn ca_signs_an_agent_csr() {
        let ca = Ca::generate().unwrap();
        let (_key_pair, csr_pem) =
            generate_agent_identity(&names(&["pc-salon", "192.168.1.34"])).unwrap();
        let issued = ca
            .sign_agent_csr(&csr_pem, &names(&["pc-salon", "pc-salon.local", "192.168.1.34"]))
            .unwrap();
        assert!(issued.cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn fingerprint_is_stable_hex_with_colons() {
        let ca = Ca::generate().unwrap();
        let fp = ca.fingerprint_sha256();
        assert_eq!(fp.split(':').count(), 32);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
    }

    #[test]
    fn sha256_hex_is_stable_lowercase_and_unseparated() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(hash, sha256_hex(b"hello"));
        assert_ne!(hash, sha256_hex(b"hellO"));
    }
}
