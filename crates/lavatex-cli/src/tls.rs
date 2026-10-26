//! Local HTTPS for the PowerPoint add-in.
//!
//! Office only loads add-in pages over HTTPS, so the helper also listens on
//! `https://localhost:<port + 1>`. `lavatex setup powerpoint` creates the
//! certificate once:
//!
//! - a CA that is name-constrained to `localhost`, `127.0.0.1` and `::1`, so
//!   even if it leaked it could not vouch for any other site. Its private key
//!   is discarded right after signing; running the setup again makes a new one;
//! - a server certificate for those names, signed by that CA.
//!
//! Only the CA certificate is added to the user's trust store.

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose, NameConstraints,
    SanType, SerialNumber,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Common name of the CA; also used to find and remove it from trust stores.
pub const CA_COMMON_NAME: &str = "LavaTeX local helper";
pub const CA_FILE: &str = "ca.pem";
pub const CERT_FILE: &str = "cert.pem";
pub const KEY_FILE: &str = "key.pem";
const VALID_YEARS: i32 = 10;

/// Folder holding the helper's certificate: `<config dir>/tls`.
pub fn tls_dir() -> Option<PathBuf> {
    lavatex_core::config::config_dir().map(|d| d.join("tls"))
}

/// Whether a certificate has been created (by `lavatex setup powerpoint`).
pub fn is_configured(dir: &Path) -> bool {
    dir.join(CERT_FILE).is_file() && dir.join(KEY_FILE).is_file()
}

fn random_serial() -> SerialNumber {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS random number generator");
    bytes[0] &= 0x7f; // keep it positive
    SerialNumber::from(bytes.to_vec())
}

fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(path)?.write_all(contents.as_bytes())
}

/// Create a new CA and server certificate in `dir`, replacing any old ones.
/// Returns the path of the CA certificate, which the caller should trust.
pub fn generate(dir: &Path) -> Result<PathBuf, String> {
    let err = |e: rcgen::Error| format!("cannot create the certificate: {e}");
    let year = current_year();
    let not_before = rcgen::date_time_ymd(year - 1, 1, 1);
    let not_after = rcgen::date_time_ymd(year + VALID_YEARS, 1, 1);

    let ca_key = KeyPair::generate().map_err(err)?;
    let mut ca = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, CA_COMMON_NAME);
    name.push(DnType::OrganizationName, "LavaTeX");
    ca.distinguished_name = name;
    ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::DnsName("localhost".into()),
            GeneralSubtree::IpAddress(CidrSubnet::V4([127, 0, 0, 1], [255; 4])),
            GeneralSubtree::IpAddress(CidrSubnet::V6(Ipv6Addr::LOCALHOST.octets(), [255; 16])),
        ],
        excluded_subtrees: vec![],
    });
    ca.serial_number = Some(random_serial());
    ca.not_before = not_before;
    ca.not_after = not_after;
    let ca_cert = ca.self_signed(&ca_key).map_err(err)?;

    let mut leaf = CertificateParams::new(vec!["localhost".to_string()]).map_err(err)?;
    leaf.subject_alt_names.extend([
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ]);
    let mut leaf_name = DistinguishedName::new();
    leaf_name.push(DnType::CommonName, "localhost");
    leaf.distinguished_name = leaf_name;
    leaf.is_ca = IsCa::ExplicitNoCa;
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf.use_authority_key_identifier_extension = true;
    leaf.serial_number = Some(random_serial());
    leaf.not_before = not_before;
    leaf.not_after = not_after;
    let leaf_key = KeyPair::generate().map_err(err)?;
    let leaf_cert = leaf.signed_by(&leaf_key, &ca_cert, &ca_key).map_err(err)?;
    drop(ca_key); // never stored: nothing else can ever be signed with it

    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let io = |p: &Path, e: std::io::Error| format!("cannot write {}: {e}", p.display());
    let ca_path = dir.join(CA_FILE);
    std::fs::write(&ca_path, ca_cert.pem()).map_err(|e| io(&ca_path, e))?;
    let cert_path = dir.join(CERT_FILE);
    std::fs::write(&cert_path, format!("{}{}", leaf_cert.pem(), ca_cert.pem()))
        .map_err(|e| io(&cert_path, e))?;
    let key_path = dir.join(KEY_FILE);
    write_private(&key_path, &leaf_key.serialize_pem()).map_err(|e| io(&key_path, e))?;
    Ok(ca_path)
}

/// TLS settings for the helper's HTTPS listener.
pub fn server_config(dir: &Path) -> Result<Arc<rustls::ServerConfig>, String> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(dir.join(CERT_FILE))
        .and_then(|it| it.collect::<Result<_, _>>())
        .map_err(|e| format!("cannot read {}: {e}", dir.join(CERT_FILE).display()))?;
    let key = PrivateKeyDer::from_pem_file(dir.join(KEY_FILE))
        .map_err(|e| format!("cannot read {}: {e}", dir.join(KEY_FILE).display()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("invalid certificate: {e}"))?;
    Ok(Arc::new(config))
}

/// A TLS client that trusts only the CA in `dir` (used to verify the setup).
pub fn client_config(dir: &Path) -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    let ca = CertificateDer::from_pem_file(dir.join(CA_FILE)).map_err(|e| e.to_string())?;
    roots.add(ca).map_err(|e| e.to_string())?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// GET `path` from `https://localhost:<port>`, verifying the certificate
/// against our CA. Returns (status, body).
pub fn https_get(dir: &Path, port: u16, path: &str) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    let config = client_config(dir)?;
    let name = rustls::pki_types::ServerName::try_from("localhost").expect("valid name");
    let conn = rustls::ClientConnection::new(config, name).map_err(|e| e.to_string())?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let tcp = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    write!(
        tls,
        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    match tls.read_to_end(&mut response) {
        Ok(_) => {}
        // Some servers close without close_notify; the response is complete.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !response.is_empty() => {}
        Err(e) => return Err(e.to_string()),
    }
    let text = String::from_utf8_lossy(&response);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("invalid HTTP response")?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_loadable_certificate() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_configured(dir.path()));
        let ca = generate(dir.path()).unwrap();
        assert!(ca.is_file());
        assert!(is_configured(dir.path()));
        server_config(dir.path()).unwrap();
        client_config(dir.path()).unwrap();
        let ca_pem = std::fs::read_to_string(ca).unwrap();
        assert!(ca_pem.contains("BEGIN CERTIFICATE"));
        let cert_pem = std::fs::read_to_string(dir.path().join(CERT_FILE)).unwrap();
        assert_eq!(cert_pem.matches("BEGIN CERTIFICATE").count(), 2);
        assert!(!std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("ca-key")));
    }
}
