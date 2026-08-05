use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use super::{
    CatalogError, CatalogSignatureEnvelope, CompatibilityCatalog, SIGNATURE_SCHEMA_VERSION,
};

#[derive(Clone, Default)]
pub struct CatalogKeyring {
    keys: BTreeMap<String, VerifyingKey>,
}

impl CatalogKeyring {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        key_id: impl Into<String>,
        public_key: &[u8],
    ) -> Result<(), CatalogError> {
        let bytes: [u8; 32] = public_key
            .try_into()
            .map_err(|_| CatalogError::Signature("Ed25519 public key must be 32 bytes".into()))?;
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|error| CatalogError::Signature(error.to_string()))?;
        self.keys.insert(key_id.into(), key);
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, key_id: &str) -> bool {
        self.keys.contains_key(key_id)
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedCatalog {
    pub catalog: CompatibilityCatalog,
    pub digest: String,
    pub signer_key_ids: Vec<String>,
    pub exact_bytes: Vec<u8>,
}

impl VerifiedCatalog {
    pub fn verify(
        catalog_bytes: &[u8],
        envelope_bytes: &[u8],
        keyring: &CatalogKeyring,
    ) -> Result<Self, CatalogError> {
        let envelope: CatalogSignatureEnvelope = serde_json::from_slice(envelope_bytes)
            .map_err(|error| CatalogError::Signature(error.to_string()))?;
        if envelope.schema_version != SIGNATURE_SCHEMA_VERSION {
            return Err(CatalogError::Signature(format!(
                "unsupported signature schema version {}",
                envelope.schema_version
            )));
        }
        if envelope.signatures.is_empty() {
            return Err(CatalogError::Signature(
                "signature envelope contains no signatures".into(),
            ));
        }
        let digest = format!("{:x}", Sha256::digest(catalog_bytes));
        if envelope.catalog_sha256 != digest {
            return Err(CatalogError::Signature(
                "Catalog digest does not match signature envelope".into(),
            ));
        }

        let mut verified = Vec::new();
        let mut trusted_seen = false;
        for candidate in envelope.signatures {
            let Some(key) = keyring.keys.get(&candidate.key_id) else {
                continue;
            };
            trusted_seen = true;
            let bytes = STANDARD.decode(&candidate.signature).map_err(|error| {
                CatalogError::Signature(format!(
                    "signature {} is not valid base64: {error}",
                    candidate.key_id
                ))
            })?;
            let signature = Signature::from_slice(&bytes).map_err(|error| {
                CatalogError::Signature(format!(
                    "signature {} has invalid length: {error}",
                    candidate.key_id
                ))
            })?;
            if key.verify(catalog_bytes, &signature).is_ok() {
                verified.push(candidate.key_id);
            }
        }
        if verified.is_empty() {
            let reason = if trusted_seen {
                "no trusted signature verified"
            } else {
                "signature envelope contains no trusted key ID"
            };
            return Err(CatalogError::Signature(reason.into()));
        }

        Ok(Self {
            catalog: CompatibilityCatalog::from_json(catalog_bytes)?,
            digest,
            signer_key_ids: verified,
            exact_bytes: catalog_bytes.to_vec(),
        })
    }
}
