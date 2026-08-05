use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
};

const MAX_TEXT_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct FileSystemHost {
    root: PathBuf,
    allow_writes: bool,
}

impl FileSystemHost {
    pub fn new(root: PathBuf, allow_writes: bool) -> std::io::Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(root)?,
            allow_writes,
        })
    }

    pub async fn read(
        &self,
        request: ReadTextFileRequest,
    ) -> std::result::Result<ReadTextFileResponse, agent_client_protocol::Error> {
        let path = self.resolve_existing(&request.path)?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(protocol_io_error)?;
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            return Err(protocol_error("file exceeds the 8 MiB host-service limit"));
        }
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(protocol_io_error)?;
        let start = request.line.unwrap_or(1).saturating_sub(1) as usize;
        let limit = request.limit.map_or(usize::MAX, |value| value as usize);
        let selected = content
            .lines()
            .skip(start)
            .take(limit)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ReadTextFileResponse::new(selected))
    }

    pub async fn write(
        &self,
        request: WriteTextFileRequest,
    ) -> std::result::Result<WriteTextFileResponse, agent_client_protocol::Error> {
        if !self.allow_writes {
            return Err(protocol_error("filesystem writes are denied by policy"));
        }
        if request.content.len() as u64 > MAX_TEXT_FILE_BYTES {
            return Err(protocol_error("write exceeds the 8 MiB host-service limit"));
        }
        let path = self.resolve_for_write(&request.path)?;
        tokio::fs::write(path, request.content)
            .await
            .map_err(protocol_io_error)?;
        Ok(WriteTextFileResponse::new())
    }

    fn resolve_existing(
        &self,
        requested: &Path,
    ) -> std::result::Result<PathBuf, agent_client_protocol::Error> {
        let canonical = std::fs::canonicalize(requested).map_err(protocol_io_error)?;
        self.ensure_within_root(canonical)
    }

    fn resolve_for_write(
        &self,
        requested: &Path,
    ) -> std::result::Result<PathBuf, agent_client_protocol::Error> {
        if requested.exists() {
            return self.resolve_existing(requested);
        }
        let parent = requested
            .parent()
            .ok_or_else(|| protocol_error("write path has no parent"))?;
        let canonical_parent = std::fs::canonicalize(parent).map_err(protocol_io_error)?;
        self.ensure_within_root(
            canonical_parent.join(
                requested
                    .file_name()
                    .ok_or_else(|| protocol_error("write path has no file name"))?,
            ),
        )
    }

    fn ensure_within_root(
        &self,
        canonical: PathBuf,
    ) -> std::result::Result<PathBuf, agent_client_protocol::Error> {
        if canonical.starts_with(&self.root) {
            Ok(canonical)
        } else {
            Err(protocol_error("filesystem request escapes the Agent root"))
        }
    }
}

fn protocol_io_error(error: std::io::Error) -> agent_client_protocol::Error {
    protocol_error(error.to_string())
}

fn protocol_error(message: impl ToString) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn rejects_reads_outside_the_canonical_root() {
        let root = std::env::current_dir().unwrap();
        let host = FileSystemHost::new(root, false).unwrap();
        let request = ReadTextFileRequest::new("session", PathBuf::from("/etc/hosts"));
        assert!(host.read(request).await.is_err());
    }

    #[tokio::test]
    async fn default_policy_rejects_writes() {
        let root = std::env::current_dir().unwrap();
        let host = FileSystemHost::new(root.clone(), false).unwrap();
        let request = WriteTextFileRequest::new("session", root.join("denied.txt"), "no");
        assert!(host.write(request).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_cannot_escape_the_agent_root_for_reads_or_writes() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("agentmux-fs-{}", Uuid::now_v7()));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let host = FileSystemHost::new(root.clone(), true).unwrap();
        assert!(
            host.read(ReadTextFileRequest::new(
                "session",
                root.join("escape/secret.txt"),
            ))
            .await
            .is_err()
        );
        assert!(
            host.write(WriteTextFileRequest::new(
                "session",
                root.join("escape/new.txt"),
                "blocked",
            ))
            .await
            .is_err()
        );
        assert!(!outside.join("new.txt").exists());
        std::fs::remove_dir_all(base).unwrap();
    }
}
