use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub trait SecureJoinExt: AsRef<Path> {
    fn secure_join(&self, untrusted: impl AsRef<Path>) -> io::Result<PathBuf> {
        let root = self.as_ref();
        if !root.exists() {
            return Err(io::Error::other("base directory does not exist"));
        }

        let untrusted = untrusted.as_ref();
        if untrusted
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(io::Error::other("path must not container '..' or prefix"));
        }

        let path = root.join(untrusted);
        let canonical_path =
            dunce::canonicalize(&path).inspect_err(|e| eprintln!("canon;:{e:?}"))?;

        if !canonical_path.starts_with(root) {
            return Err(io::Error::other("path escapes the base directory"));
        }

        Ok(canonical_path)
    }
}

impl SecureJoinExt for PathBuf {}
impl SecureJoinExt for &Path {}
