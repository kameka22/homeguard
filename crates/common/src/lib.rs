pub mod aliases;
pub mod hosts;
pub mod pki;
pub mod policy;
pub mod tls;
pub mod wire;

pub use aliases::{default_catalog, expand_domains, AliasEntry};
pub use hosts::{apply_block, extract_block, is_valid_domain, normalize_domain, ApplyResult};
pub use pki::{fingerprint_of_pem, generate_agent_identity, sha256_hex, Ca, CertifiedKey, IssuedCert, PkiError};
pub use policy::{Day, Exception, Policy, PolicyError, Protection, Rule, TimeWindow};
pub use tls::{
    client_config_mtls, client_config_pinned_ca, install_crypto_provider, server_config_mtls,
    server_config_tls_no_client_auth, TlsError,
};
pub use wire::{
    AgentInfo, AgentStatus, EnrollRequest, EnrollResponse, EventEntry, EventKind, EventsPage,
};
