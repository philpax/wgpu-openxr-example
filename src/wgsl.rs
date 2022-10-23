//! Implements a very simple preprocessor to embed other WGSL files.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context;

/// Preprocesses the given `current_file` within the `files`, and returns the preprocessed file.
/// `files` must contain non-preprocessed files.
///
/// This is _not_ a robust preprocessor. It's the bare minimum to make this example work.
/// This *will* fall down at the first hurdle.
pub fn preprocess(files: &HashMap<PathBuf, String>, current_file: &str) -> anyhow::Result<String> {
    let mut current_file = current_file.to_string();
    while current_file.contains("#include") {
        current_file = current_file
            .lines()
            .map(|l| match l.strip_prefix("#include ") {
                Some(filename) => Ok(files
                    .get(&PathBuf::from(filename))
                    .context("failed to find file")?
                    .as_ref()),
                None => Ok(l),
            })
            .collect::<anyhow::Result<Vec<&str>>>()?
            .join("\n");
    }
    Ok(current_file)
}

/// A helper for [preprocess] that wraps it with some files to use for state.
pub struct Preprocessor {
    files: HashMap<PathBuf, String>,
}
impl Preprocessor {
    /// Create a [Preprocessor] from the given `path`.
    pub fn from_directory(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            files: std::fs::read_dir(path)?
                .filter_map(Result::ok)
                .map(|de| de.path())
                .filter(|p| p.extension().unwrap_or_default() == "wgsl")
                .map(|p| {
                    Ok((
                        PathBuf::from(
                            p.file_name().ok_or_else(|| {
                                std::io::Error::from(std::io::ErrorKind::NotFound)
                            })?,
                        ),
                        std::fs::read_to_string(&p)?,
                    ))
                })
                .collect::<std::io::Result<_>>()?,
        })
    }

    /// Runs [crate::preprocess] on the given `filename`, assuming that it is within the files that
    /// initialized this preprocessor.
    pub fn preprocess(&self, filename: impl AsRef<Path>) -> anyhow::Result<String> {
        preprocess(
            &self.files,
            self.files
                .get(filename.as_ref())
                .context("file not present")?
                .as_str(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::preprocess;

    #[test]
    fn preprocess_can_include() {
        let main_file = "#include blah.wgsl\n// and good night!";
        let files = [
            (PathBuf::from("foo.wgsl"), "// first file!".to_string()),
            (
                PathBuf::from("blah.wgsl"),
                "#include foo.wgsl\n// hello world!".to_string(),
            ),
            (PathBuf::from("main.wgsl"), main_file.to_string()),
        ]
        .into_iter()
        .collect();

        let expected_output = r#"// first file!
// hello world!
// and good night!"#;

        assert_eq!(preprocess(&files, main_file).unwrap(), expected_output);
    }
}
