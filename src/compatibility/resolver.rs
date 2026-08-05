use semver::{Version, VersionReq};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{
    ArtifactDigest, CatalogEntry, CatalogEntryState, CatalogError, CompatibilityLevel,
    DigestAlgorithm, ResolvedProviderLock, VerifiedCatalog, VersionPolicy,
};
use crate::{ObservedProvider, ProviderDriver};

pub struct ResolutionRequest<'a> {
    pub driver: &'a ProviderDriver,
    pub observed: &'a ObservedProvider,
    pub target: &'a str,
    pub policy: VersionPolicy,
    pub exact_entry: Option<&'a str>,
    pub agentmux_version: &'a Version,
    pub now: OffsetDateTime,
}

pub fn resolve_provider(
    catalog: &VerifiedCatalog,
    request: &ResolutionRequest<'_>,
) -> Result<ResolvedProviderLock, CatalogError> {
    catalog.catalog.validate_structure()?;
    let driver_entries: Vec<_> = catalog
        .catalog
        .entries
        .iter()
        .filter(|entry| {
            entry.provider == request.driver.id
                && entry.target == request.target
                && entry.driver_id == request.driver.driver_id
                && entry.driver_revision == request.driver.driver_revision
        })
        .collect();

    if driver_entries.iter().any(|entry| {
        entry.state == CatalogEntryState::Blocked && exact_match(entry, request.observed)
    }) {
        return Err(resolution(format!(
            "provider identity {} is blocked",
            request.observed.identity.display_version
        )));
    }

    let relevant: Vec<_> = driver_entries
        .into_iter()
        .filter(|entry| requirement_matches(entry, request.agentmux_version))
        .collect();

    let exact_matches: Vec<_> = relevant
        .iter()
        .copied()
        .filter(|entry| exact_match(entry, request.observed))
        .collect();
    if exact_matches.len() > 1 {
        return Err(resolution(format!(
            "provider identity {} ambiguously matches multiple Catalog entries",
            request.observed.identity.display_version
        )));
    }
    let matched_entry = exact_matches.first().copied();

    match request.policy {
        VersionPolicy::Verified => {
            ensure_catalog_fresh(catalog, request.now)?;
            let entry = matched_entry.ok_or_else(|| {
                let detail = artifact_mismatch_detail(&relevant, request.observed);
                resolution(format!(
                    "provider identity {} and artifact set are not Catalog-verified{detail}",
                    request.observed.identity.display_version,
                ))
            })?;
            verified_lock(catalog, request, entry)
        }
        VersionPolicy::Exact => {
            ensure_catalog_fresh(catalog, request.now)?;
            let requested = request
                .exact_entry
                .ok_or_else(|| resolution("exact policy requires catalog_entry"))?;
            let entry = relevant
                .iter()
                .copied()
                .find(|entry| entry.entry_id == requested)
                .ok_or_else(|| resolution(format!("Catalog entry {requested:?} is unavailable")))?;
            if !exact_match(entry, request.observed) {
                return Err(resolution(format!(
                    "installed identity/artifacts do not match Catalog entry {requested:?}"
                )));
            }
            verified_lock(catalog, request, entry)
        }
        VersionPolicy::Experimental => {
            if ensure_catalog_fresh(catalog, request.now).is_ok()
                && let Some(entry) = matched_entry
            {
                return verified_lock(catalog, request, entry);
            }
            Ok(ResolvedProviderLock {
                provider: request.driver.id,
                driver_id: request.driver.driver_id.clone(),
                driver_revision: request.driver.driver_revision,
                target: request.target.to_owned(),
                compatibility: CompatibilityLevel::Experimental,
                identity: request.observed.identity.clone(),
                artifacts: sorted_artifacts(&request.observed.artifacts),
                catalog_entry_id: None,
                catalog_sequence: None,
                catalog_digest: None,
            })
        }
    }
}

fn artifact_mismatch_detail(entries: &[&CatalogEntry], observed: &ObservedProvider) -> String {
    let Some(entry) = entries
        .iter()
        .copied()
        .find(|entry| entry.identity == observed.identity)
    else {
        return "; no entry has the observed identity".into();
    };
    let expected: std::collections::BTreeMap<_, _> = entry
        .artifacts
        .iter()
        .map(|artifact| (artifact.subject.as_str(), artifact.digest.as_str()))
        .collect();
    let actual: std::collections::BTreeMap<_, _> = observed
        .artifacts
        .iter()
        .map(|artifact| (artifact.subject.as_str(), artifact.digest.as_str()))
        .collect();
    let mut mismatches = expected
        .keys()
        .chain(actual.keys())
        .filter(|subject| expected.get(**subject) != actual.get(**subject))
        .copied()
        .collect::<Vec<_>>();
    mismatches.sort_unstable();
    mismatches.dedup();
    format!("; mismatched artifact subjects: {}", mismatches.join(", "))
}

fn verified_lock(
    catalog: &VerifiedCatalog,
    request: &ResolutionRequest<'_>,
    entry: &CatalogEntry,
) -> Result<ResolvedProviderLock, CatalogError> {
    let compatibility = match entry.state {
        CatalogEntryState::Verified => CompatibilityLevel::Verified,
        CatalogEntryState::Deprecated => {
            let not_after = entry
                .not_after
                .as_deref()
                .ok_or_else(|| resolution("deprecated entry is missing not_after"))?;
            let not_after = OffsetDateTime::parse(not_after, &Rfc3339)
                .map_err(|error| resolution(format!("invalid not_after: {error}")))?;
            if not_after <= request.now {
                return Err(resolution(format!(
                    "deprecated Catalog entry {} passed not_after",
                    entry.entry_id
                )));
            }
            CompatibilityLevel::Deprecated
        }
        CatalogEntryState::Blocked => {
            return Err(resolution(format!(
                "Catalog entry {} is blocked",
                entry.entry_id
            )));
        }
    };
    Ok(ResolvedProviderLock {
        provider: request.driver.id,
        driver_id: request.driver.driver_id.clone(),
        driver_revision: request.driver.driver_revision,
        target: request.target.to_owned(),
        compatibility,
        identity: request.observed.identity.clone(),
        artifacts: sorted_artifacts(&request.observed.artifacts),
        catalog_entry_id: Some(entry.entry_id.clone()),
        catalog_sequence: Some(catalog.catalog.sequence),
        catalog_digest: Some(catalog.digest.clone()),
    })
}

fn exact_match(entry: &CatalogEntry, observed: &ObservedProvider) -> bool {
    entry.identity == observed.identity
        && sorted_artifacts(&entry.artifacts) == sorted_artifacts(&observed.artifacts)
}

fn sorted_artifacts(artifacts: &[ArtifactDigest]) -> Vec<ArtifactDigest> {
    let mut artifacts = artifacts.to_vec();
    artifacts.sort_by(|left, right| {
        left.subject
            .cmp(&right.subject)
            .then_with(|| algorithm_name(left.algorithm).cmp(algorithm_name(right.algorithm)))
            .then_with(|| left.digest.cmp(&right.digest))
    });
    artifacts
}

const fn algorithm_name(algorithm: DigestAlgorithm) -> &'static str {
    match algorithm {
        DigestAlgorithm::Sha256 => "sha256",
    }
}

fn requirement_matches(entry: &CatalogEntry, version: &Version) -> bool {
    VersionReq::parse(&entry.agentmux_requirement)
        .is_ok_and(|requirement| requirement.matches(version))
}

fn ensure_catalog_fresh(
    catalog: &VerifiedCatalog,
    now: OffsetDateTime,
) -> Result<(), CatalogError> {
    let expires = OffsetDateTime::parse(&catalog.catalog.expires_at, &Rfc3339)
        .map_err(|error| resolution(format!("invalid Catalog expiry: {error}")))?;
    if expires <= now {
        return Err(CatalogError::Expired(catalog.catalog.expires_at.clone()));
    }
    Ok(())
}

fn resolution(message: impl Into<String>) -> CatalogError {
    CatalogError::Resolution(message.into())
}

#[must_use]
pub const fn host_target() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux"),
        all(target_arch = "aarch64", target_os = "linux")
    )))]
    {
        "unsupported-target"
    }
}
