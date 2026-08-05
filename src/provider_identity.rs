use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{PermissionPolicy, ProviderId};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DriverId(pub String);

impl DriverId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum ProbeObservation<T> {
    Observed(T),
    Unavailable(String),
    Malformed(String),
}

impl<T> ProbeObservation<T> {
    #[must_use]
    pub const fn observed(&self) -> Option<&T> {
        match self {
            Self::Observed(value) => Some(value),
            Self::Unavailable(_) | Self::Malformed(_) => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderAssertions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_sha256: Option<String>,
}

impl ProviderAssertions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.version.is_none() && self.components.is_empty() && self.launch_sha256.is_none()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version.as_deref().is_some_and(str::is_empty) {
            return Err("assertions.version must not be empty".into());
        }
        if self
            .components
            .iter()
            .any(|(name, value)| name.is_empty() || value.is_empty())
        {
            return Err("assertion component names and values must not be empty".into());
        }
        if let Some(digest) = &self.launch_sha256
            && (digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(
                "assertions.launch_sha256 must contain exactly 64 hexadecimal digits".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum AssertionResult {
    NotConfigured,
    Matched { fields: Vec<String> },
    Failed { reasons: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderImplementationInfo {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderExecutionIdentity {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub target: String,
    pub executable_path: PathBuf,
    pub launch_sha256: String,
    pub observed_version: ProbeObservation<String>,
    #[serde(default)]
    pub observed_components: BTreeMap<String, ProbeObservation<String>>,
    pub acp_protocol_version: Option<u32>,
    pub acp_agent_info: Option<ProviderImplementationInfo>,
    pub capability_digest: Option<String>,
    pub assertion_result: AssertionResult,
}

impl ProviderExecutionIdentity {
    #[must_use]
    pub fn launch_fingerprint(
        &self,
        launch_args: &[String],
        fixed_env: &BTreeMap<String, String>,
        permission_policy: PermissionPolicy,
    ) -> String {
        #[derive(Serialize)]
        struct Fingerprint<'a> {
            provider: ProviderId,
            driver_id: &'a DriverId,
            driver_revision: u32,
            executable_path: &'a Path,
            launch_sha256: &'a str,
            launch_args: &'a [String],
            fixed_env: &'a BTreeMap<String, String>,
            permission_policy: PermissionPolicy,
        }
        let bytes = serde_json::to_vec(&Fingerprint {
            provider: self.provider,
            driver_id: &self.driver_id,
            driver_revision: self.driver_revision,
            executable_path: &self.executable_path,
            launch_sha256: &self.launch_sha256,
            launch_args,
            fixed_env,
            permission_policy,
        })
        .expect("provider launch fingerprint fields are serializable");
        format!("{:x}", Sha256::digest(bytes))
    }

    #[must_use]
    pub fn summary(&self) -> ProviderIdentitySummary {
        ProviderIdentitySummary {
            provider: self.provider,
            driver_id: self.driver_id.clone(),
            driver_revision: self.driver_revision,
            target: self.target.clone(),
            executable_path: self.executable_path.clone(),
            launch_sha256: self.launch_sha256.clone(),
            observed_version: self.observed_version.clone(),
            observed_components: self.observed_components.clone(),
            acp_protocol_version: self.acp_protocol_version,
            acp_agent_info: self.acp_agent_info.clone(),
            capability_digest: self.capability_digest.clone(),
            assertion_result: self.assertion_result.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdentitySummary {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub target: String,
    pub executable_path: PathBuf,
    pub launch_sha256: String,
    pub observed_version: ProbeObservation<String>,
    #[serde(default)]
    pub observed_components: BTreeMap<String, ProbeObservation<String>>,
    pub acp_protocol_version: Option<u32>,
    pub acp_agent_info: Option<ProviderImplementationInfo>,
    pub capability_digest: Option<String>,
    pub assertion_result: AssertionResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedProvider {
    pub executable: PathBuf,
    pub launch_sha256: String,
    pub version: ProbeObservation<String>,
    pub components: BTreeMap<String, ProbeObservation<String>>,
    pub launch_file: ObservedArtifactFile,
    pub supplementary_files: Vec<ObservedArtifactFile>,
}

impl ObservedProvider {
    #[must_use]
    pub fn assertion_result(&self, assertions: &ProviderAssertions) -> AssertionResult {
        if assertions.is_empty() {
            return AssertionResult::NotConfigured;
        }
        let mut matched = Vec::new();
        let mut reasons = Vec::new();
        if let Some(expected) = &assertions.version {
            compare_observation(
                "version",
                expected,
                &self.version,
                &mut matched,
                &mut reasons,
            );
        }
        for (component, expected) in &assertions.components {
            match self.components.get(component) {
                Some(observation) => compare_observation(
                    &format!("components.{component}"),
                    expected,
                    observation,
                    &mut matched,
                    &mut reasons,
                ),
                None => reasons.push(format!("component {component:?} was not observed")),
            }
        }
        if let Some(expected) = &assertions.launch_sha256 {
            if expected.eq_ignore_ascii_case(&self.launch_sha256) {
                matched.push("launch_sha256".into());
            } else {
                reasons.push("launch_sha256 did not match the configured assertion".into());
            }
        }
        if reasons.is_empty() {
            AssertionResult::Matched { fields: matched }
        } else {
            AssertionResult::Failed { reasons }
        }
    }

    pub fn verify_unchanged(&self, assertions: &ProviderAssertions) -> std::io::Result<()> {
        self.launch_file.verify_unchanged()?;
        if !assertions.components.is_empty() {
            for file in &self.supplementary_files {
                file.verify_unchanged()?;
            }
        }
        Ok(())
    }
}

fn compare_observation(
    field: &str,
    expected: &str,
    actual: &ProbeObservation<String>,
    matched: &mut Vec<String>,
    reasons: &mut Vec<String>,
) {
    match actual {
        ProbeObservation::Observed(value) if value == expected => matched.push(field.into()),
        ProbeObservation::Observed(_) => reasons.push(format!("{field} did not match")),
        ProbeObservation::Unavailable(reason) => {
            reasons.push(format!("{field} could not be observed: {reason}"));
        }
        ProbeObservation::Malformed(reason) => {
            reasons.push(format!("{field} observation was malformed: {reason}"));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedArtifactFile {
    pub subject: String,
    pub path: PathBuf,
    pub identity: ExecutableFileIdentity,
    pub executable_required: bool,
}

impl ObservedArtifactFile {
    pub fn verify_unchanged(&self) -> std::io::Result<()> {
        self.identity
            .verify_artifact_path(&self.path, self.executable_required)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableFileIdentity {
    pub owner: u32,
    pub mode: u32,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
}

impl ExecutableFileIdentity {
    pub fn from_artifact_path(path: &Path, executable_required: bool) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider artifact is not a regular file",
            ));
        }
        let owner = metadata.uid();
        if owner != unsafe { libc::geteuid() } && owner != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider artifact must be owned by the current user or root",
            ));
        }
        let mode = metadata.mode();
        if mode & 0o022 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider artifact must not be group/world writable",
            ));
        }
        if executable_required && mode & 0o111 == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider launch artifact is not executable",
            ));
        }
        Ok(Self {
            owner,
            mode,
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }

    pub fn verify_artifact_path(
        &self,
        path: &Path,
        executable_required: bool,
    ) -> std::io::Result<()> {
        let current = Self::from_artifact_path(path, executable_required)?;
        if &current == self {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider artifact changed after observation",
            ))
        }
    }
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
