use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub trait TrustedPathExt: AsRef<Path> {
    // makes sure path is inside base directory
    fn secure_join(&self, untrusted: impl AsRef<Path>) -> io::Result<PathBuf> {
        let canonical_root = dunce::canonicalize(self.as_ref())?;

        if !canonical_root.exists() {
            return Err(io::Error::other("base directory does not exist"));
        }

        let untrusted = untrusted.as_ref();
        if untrusted
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(io::Error::other("path must not contain '..' or prefix"));
        }

        let path = canonical_root.join(untrusted);
        let canonical_path = dunce::canonicalize(&path)?;

        if !canonical_path.starts_with(&canonical_root) {
            return Err(io::Error::other("path escapes the base directory"));
        }

        Ok(canonical_path)
    }

    fn canonicalize_trusted(&self, absolute_untrusted: impl AsRef<Path>) -> io::Result<PathBuf> {
        let canonical_root = dunce::canonicalize(self.as_ref())?;

        if !canonical_root.exists() {
            return Err(io::Error::other("base directory does not exist"));
        }

        let canonical_untrusted = dunce::canonicalize(absolute_untrusted.as_ref())?;

        if !canonical_untrusted.starts_with(canonical_root) {
            return Err(io::Error::other("path is outside the base directory"));
        }

        Ok(canonical_untrusted)
    }

    fn build_trusted_path(&self, untrusted: impl AsRef<Path>) -> io::Result<PathBuf> {
        let canonical_root = dunce::canonicalize(self.as_ref())?;

        if !canonical_root.exists() {
            return Err(io::Error::other("base directory does not exist"));
        }

        let untrusted = untrusted.as_ref();
        if untrusted
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(io::Error::other("path must not contain '..' or prefix"));
        }

        let path = canonical_root.join(untrusted);

        Ok(path)
    }
}

impl TrustedPathExt for PathBuf {}
impl TrustedPathExt for &Path {}
