use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ProviderId;

pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const SIGNATURE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DriverId(pub String);

impl DriverId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolLock {
    #[serde(rename = "v1")]
    StableV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdentity {
    pub display_version: String,
    pub normalized_version: String,
    #[serde(default)]
    pub components: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithm {
    Sha256,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDigest {
    pub subject: String,
    pub algorithm: DigestAlgorithm,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryState {
    Verified,
    Deprecated,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationEvidence {
    pub suite_version: u32,
    pub tested_at: String,
    pub evidence_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub entry_id: String,
    pub provider: ProviderId,
    pub target: String,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub agentmux_requirement: String,
    pub protocol: ProtocolLock,
    pub identity: ProviderIdentity,
    pub artifacts: Vec<ArtifactDigest>,
    pub state: CatalogEntryState,
    pub not_after: Option<String>,
    pub qualification: QualificationEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogChannel {
    pub provider: ProviderId,
    pub target: String,
    pub name: String,
    pub entry_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityCatalog {
    pub schema_version: u32,
    pub catalog_id: String,
    pub sequence: u64,
    pub generated_at: String,
    pub expires_at: String,
    pub entries: Vec<CatalogEntry>,
    pub channels: Vec<CatalogChannel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSignature {
    pub key_id: String,
    pub algorithm: SignatureAlgorithm,
    pub signature: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithm {
    Ed25519,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSignatureEnvelope {
    pub schema_version: u32,
    pub catalog_sha256: String,
    pub signatures: Vec<CatalogSignature>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionPolicy {
    #[default]
    Verified,
    Exact,
    Experimental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityLevel {
    Verified,
    Deprecated,
    Experimental,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProviderLock {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub target: String,
    pub compatibility: CompatibilityLevel,
    pub identity: ProviderIdentity,
    pub artifacts: Vec<ArtifactDigest>,
    pub catalog_entry_id: Option<String>,
    pub catalog_sequence: Option<u64>,
    pub catalog_digest: Option<String>,
}

impl ResolvedProviderLock {
    #[must_use]
    pub fn canonical_digest(&self) -> String {
        let bytes = serde_json::to_vec(self)
            .expect("ResolvedProviderLock contains only infallibly serializable fields");
        format!("{:x}", Sha256::digest(bytes))
    }

    #[must_use]
    pub fn summary(&self) -> ProviderLockSummary {
        ProviderLockSummary {
            provider: self.provider,
            compatibility: self.compatibility,
            display_version: self.identity.display_version.clone(),
            components: self.identity.components.clone(),
            artifact_digests: self.artifacts.clone(),
            driver_id: self.driver_id.clone(),
            driver_revision: self.driver_revision,
            catalog_entry_id: self.catalog_entry_id.clone(),
            catalog_sequence: self.catalog_sequence,
            catalog_digest: self.catalog_digest.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLockSummary {
    pub provider: ProviderId,
    pub compatibility: CompatibilityLevel,
    pub display_version: String,
    pub components: BTreeMap<String, String>,
    pub artifact_digests: Vec<ArtifactDigest>,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub catalog_entry_id: Option<String>,
    pub catalog_sequence: Option<u64>,
    pub catalog_digest: Option<String>,
}
