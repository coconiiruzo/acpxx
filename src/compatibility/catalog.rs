use std::collections::{BTreeMap, BTreeSet};

use semver::{Version, VersionReq};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{
    CATALOG_SCHEMA_VERSION, CatalogEntry, CatalogEntryState, CompatibilityCatalog, DigestAlgorithm,
    DriverId,
};

#[derive(Clone, Debug)]
pub struct CatalogValidationContext<'a> {
    pub target: &'a str,
    pub agentmux_version: &'a Version,
    pub drivers: &'a BTreeMap<DriverId, u32>,
    pub now: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("invalid Catalog JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unsupported Catalog schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid Catalog: {0}")]
    Invalid(String),
    #[error("Catalog expired at {0}")]
    Expired(String),
    #[error("Catalog sequence {candidate} does not exceed accepted sequence {accepted}")]
    Rollback { candidate: u64, accepted: u64 },
    #[error("Catalog signature verification failed: {0}")]
    Signature(String),
    #[error("Catalog cache failure: {0}")]
    Cache(String),
    #[error("provider resolution failed: {0}")]
    Resolution(String),
}

impl CompatibilityCatalog {
    pub fn from_json(bytes: &[u8]) -> Result<Self, CatalogError> {
        let catalog: Self = serde_json::from_slice(bytes)?;
        catalog.validate_structure()?;
        Ok(catalog)
    }

    pub fn validate_structure(&self) -> Result<(), CatalogError> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchema(self.schema_version));
        }
        validate_token("catalog_id", &self.catalog_id)?;
        if self.sequence == 0 {
            return Err(invalid("sequence must be greater than zero"));
        }
        let generated = timestamp("generated_at", &self.generated_at)?;
        let expires = timestamp("expires_at", &self.expires_at)?;
        if expires <= generated {
            return Err(invalid("expires_at must be later than generated_at"));
        }
        if self.entries.is_empty() {
            return Err(invalid("entries must not be empty"));
        }

        let mut entry_ids = BTreeSet::new();
        for entry in &self.entries {
            validate_entry(entry)?;
            if !entry_ids.insert(entry.entry_id.as_str()) {
                return Err(invalid(format!("duplicate entry_id {:?}", entry.entry_id)));
            }
        }

        let entries: BTreeMap<_, _> = self
            .entries
            .iter()
            .map(|entry| (entry.entry_id.as_str(), entry))
            .collect();
        let mut channels = BTreeSet::new();
        for channel in &self.channels {
            validate_token("channel name", &channel.name)?;
            if channel.target.trim().is_empty() {
                return Err(invalid("channel target must not be empty"));
            }
            if !channels.insert((
                channel.provider,
                channel.target.as_str(),
                channel.name.as_str(),
            )) {
                return Err(invalid(format!(
                    "duplicate channel {}/{}/{}",
                    channel.provider, channel.target, channel.name
                )));
            }
            let entry = entries.get(channel.entry_id.as_str()).ok_or_else(|| {
                invalid(format!(
                    "channel {} references missing entry {}",
                    channel.name, channel.entry_id
                ))
            })?;
            if entry.provider != channel.provider || entry.target != channel.target {
                return Err(invalid(format!(
                    "channel {} crosses provider or target boundary",
                    channel.name
                )));
            }
            if entry.state == CatalogEntryState::Blocked {
                return Err(invalid(format!(
                    "channel {} references blocked entry {}",
                    channel.name, channel.entry_id
                )));
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, context: &CatalogValidationContext<'_>) -> Result<(), CatalogError> {
        self.validate_structure()?;
        if timestamp("expires_at", &self.expires_at)? <= context.now {
            return Err(CatalogError::Expired(self.expires_at.clone()));
        }
        for entry in &self.entries {
            if entry.target != context.target {
                continue;
            }
            let Some(revision) = context.drivers.get(&entry.driver_id) else {
                continue;
            };
            if revision != &entry.driver_revision {
                continue;
            }
            let requirement = VersionReq::parse(&entry.agentmux_requirement).map_err(|error| {
                invalid(format!(
                    "entry {} has invalid agentmux_requirement: {error}",
                    entry.entry_id
                ))
            })?;
            if !requirement.matches(context.agentmux_version) {
                return Err(invalid(format!(
                    "entry {} does not support agentmux {}",
                    entry.entry_id, context.agentmux_version
                )));
            }
        }
        Ok(())
    }
}

fn validate_entry(entry: &CatalogEntry) -> Result<(), CatalogError> {
    validate_token("entry_id", &entry.entry_id)?;
    validate_token("driver_id", &entry.driver_id.0)?;
    if entry.target.trim().is_empty() {
        return Err(invalid(format!(
            "entry {} target must not be empty",
            entry.entry_id
        )));
    }
    if entry.driver_revision == 0 {
        return Err(invalid(format!(
            "entry {} driver_revision must be greater than zero",
            entry.entry_id
        )));
    }
    VersionReq::parse(&entry.agentmux_requirement).map_err(|error| {
        invalid(format!(
            "entry {} has invalid agentmux_requirement: {error}",
            entry.entry_id
        ))
    })?;
    if entry.identity.display_version.trim().is_empty()
        || entry.identity.normalized_version.trim().is_empty()
    {
        return Err(invalid(format!(
            "entry {} identity versions must not be empty",
            entry.entry_id
        )));
    }
    for (name, value) in &entry.identity.components {
        validate_token("identity component", name)?;
        if value.trim().is_empty() {
            return Err(invalid(format!(
                "entry {} component {name} must not be empty",
                entry.entry_id
            )));
        }
    }
    if entry.artifacts.is_empty() {
        return Err(invalid(format!(
            "entry {} artifacts must not be empty",
            entry.entry_id
        )));
    }
    let mut subjects = BTreeSet::new();
    for artifact in &entry.artifacts {
        validate_token("artifact subject", &artifact.subject)?;
        if !subjects.insert(artifact.subject.as_str()) {
            return Err(invalid(format!(
                "entry {} repeats artifact subject {}",
                entry.entry_id, artifact.subject
            )));
        }
        match artifact.algorithm {
            DigestAlgorithm::Sha256 => validate_sha256(&artifact.digest)?,
        }
    }
    if entry.state == CatalogEntryState::Deprecated && entry.not_after.is_none() {
        return Err(invalid(format!(
            "deprecated entry {} requires not_after",
            entry.entry_id
        )));
    }
    if let Some(not_after) = &entry.not_after {
        timestamp("not_after", not_after)?;
    }
    if entry.qualification.suite_version == 0 {
        return Err(invalid(format!(
            "entry {} qualification suite_version must be greater than zero",
            entry.entry_id
        )));
    }
    timestamp("qualification.tested_at", &entry.qualification.tested_at)?;
    let evidence = entry
        .qualification
        .evidence_digest
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid("qualification evidence_digest must use sha256"))?;
    validate_sha256(evidence)
}

pub fn validate_sha256(value: &str) -> Result<(), CatalogError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("SHA-256 digest must be 64 lowercase hex digits"));
    }
    Ok(())
}

fn validate_token(field: &str, value: &str) -> Result<(), CatalogError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(invalid(format!("{field} contains unsupported characters")));
    }
    Ok(())
}

fn timestamp(field: &str, value: &str) -> Result<OffsetDateTime, CatalogError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| invalid(format!("{field} is not RFC3339: {error}")))
}

fn invalid(message: impl Into<String>) -> CatalogError {
    CatalogError::Invalid(message.into())
}
