//! Read-only access to the project's files, and runtime detection.
//!
//! The core never touches the file system directly: it reads through [`Project`], so tests
//! can use [`MemProject`] and the output stays a pure function of the inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Project files, addressed by `/`-separated paths relative to the folder holding the spec.
pub trait Project {
    fn read(&self, path: &str) -> Option<String>;
    fn is_dir(&self, path: &str) -> bool;
    fn is_file(&self, path: &str) -> bool;
    /// Names of the direct sub-folders of `path`, sorted.
    fn subdirs(&self, path: &str) -> Vec<String>;
}

/// The real file system under a root folder.
#[derive(Debug, Clone)]
pub struct DirProject {
    root: PathBuf,
}

impl DirProject {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, path: &str) -> PathBuf {
        if path == "." { self.root.clone() } else { self.root.join(Path::new(path)) }
    }
}

impl Project for DirProject {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.path(path)).ok()
    }

    fn is_dir(&self, path: &str) -> bool {
        self.path(path).is_dir()
    }

    fn is_file(&self, path: &str) -> bool {
        self.path(path).is_file()
    }

    fn subdirs(&self, path: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.path(path)) else { return Vec::new() };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }
}

/// An in-memory project for tests.
#[derive(Debug, Clone, Default)]
pub struct MemProject {
    files: BTreeMap<String, String>,
    dirs: BTreeSet<String>,
}

impl MemProject {
    pub fn new() -> Self {
        let mut project = Self::default();
        project.dirs.insert(".".to_owned());
        project
    }

    #[must_use]
    pub fn with_file(mut self, path: &str, contents: &str) -> Self {
        let mut parent = Path::new(path).parent();
        while let Some(dir) = parent.filter(|p| !p.as_os_str().is_empty()) {
            self.dirs.insert(dir.to_string_lossy().replace('\\', "/"));
            parent = dir.parent();
        }
        self.files.insert(path.to_owned(), contents.to_owned());
        self
    }

    #[must_use]
    pub fn with_dir(mut self, path: &str) -> Self {
        self.dirs.insert(path.to_owned());
        self
    }
}

impl Project for MemProject {
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn is_dir(&self, path: &str) -> bool {
        self.dirs.contains(path)
    }

    fn is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn subdirs(&self, path: &str) -> Vec<String> {
        let prefix = if path == "." { String::new() } else { format!("{path}/") };
        self.dirs
            .iter()
            .filter_map(|d| d.strip_prefix(&prefix))
            .filter(|rest| !rest.is_empty() && *rest != "." && !rest.contains('/'))
            .map(str::to_owned)
            .collect()
    }
}

/// Normalises a spec path: `/` separators, no leading `./`, no trailing `/`; `.` for the root.
pub fn normalize_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() { ".".to_owned() } else { parts.join("/") }
}

/// `join("api", "package.json")` → `api/package.json`; `join(".", x)` → `x`.
pub fn join(dir: &str, file: &str) -> String {
    if dir == "." { file.to_owned() } else { format!("{dir}/{file}") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Node,
    Python,
    Go,
    Rust,
}

/// A language runtime such as `node22` or `python3.13`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Runtime {
    pub language: Language,
    /// `22`, `3.13`, `1.23`; `None` for plain `rust`.
    pub version: Option<String>,
}

impl Runtime {
    /// Parses the `runtime:` field. The schema has already checked the format.
    pub fn parse(text: &str) -> Option<Self> {
        let (language, version) = [("node", Language::Node), ("python", Language::Python), ("go", Language::Go)]
            .into_iter()
            .find_map(|(prefix, lang)| text.strip_prefix(prefix).map(|v| (lang, Some(v.to_owned()))))
            .or_else(|| text.strip_prefix("rust").map(|v| (Language::Rust, (!v.is_empty()).then(|| v.to_owned()))))?;
        if version.as_deref().is_some_and(|v| v.is_empty() || !v.chars().all(|c| c.is_ascii_digit() || c == '.')) {
            return None;
        }
        Some(Self { language, version })
    }

    fn new(language: Language, version: &str) -> Self {
        Self { language, version: Some(version.to_owned()) }
    }
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.language {
            Language::Node => "node",
            Language::Python => "python",
            Language::Go => "go",
            Language::Rust => "rust",
        };
        write!(f, "{prefix}{}", self.version.as_deref().unwrap_or(""))
    }
}

impl Serialize for Runtime {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Default versions used when a manifest doesn't say.
pub const DEFAULT_NODE: &str = "22";
pub const DEFAULT_PYTHON: &str = "3.13";
pub const DEFAULT_GO: &str = "1.23";

/// Manifests that identify a service folder, in detection order.
pub const MANIFESTS: [&str; 4] = ["package.json", "pyproject.toml", "go.mod", "Cargo.toml"];

/// Detects the runtime of the code in `dir` from its manifest.
/// Returns the runtime and the manifest it came from.
pub fn detect_runtime(project: &dyn Project, dir: &str) -> Option<(Runtime, String)> {
    MANIFESTS.iter().find_map(|manifest| {
        let path = join(dir, manifest);
        let text = project.read(&path)?;
        let runtime = match *manifest {
            "package.json" => Runtime::new(Language::Node, &node_version(&text).unwrap_or_else(|| DEFAULT_NODE.into())),
            "pyproject.toml" => {
                Runtime::new(Language::Python, &python_version(&text).unwrap_or_else(|| DEFAULT_PYTHON.into()))
            }
            "go.mod" => Runtime::new(Language::Go, &go_version(&text).unwrap_or_else(|| DEFAULT_GO.into())),
            _ => Runtime { language: Language::Rust, version: None },
        };
        Some((runtime, path))
    })
}

/// The first number in `engines.node`, for example `>=20.1` → `20`.
fn node_version(package_json: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(package_json).ok()?;
    let range = json.get("engines")?.get("node")?.as_str()?;
    let digits: String = range.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// `requires-python = ">=3.12"` → `3.12`.
fn python_version(pyproject: &str) -> Option<String> {
    let line = pyproject.lines().find(|l| l.trim_start().starts_with("requires-python"))?;
    let value = line.split_once('=')?.1;
    let start = value.find("3.")?;
    let version: String = value[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next().filter(|p| !p.is_empty())?))
}

/// `go 1.23.4` → `1.23`.
fn go_version(go_mod: &str) -> Option<String> {
    let version = go_mod.lines().find_map(|l| l.trim().strip_prefix("go "))?.trim();
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}

/// Folders skipped when looking for services.
const IGNORED_DIRS: [&str; 6] = ["node_modules", "target", "vendor", "dist", "build", "__pycache__"];

/// A folder that looks like a service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Discovered {
    /// `/`-separated path relative to the project root; `.` for the root itself.
    pub dir: String,
    pub runtime: Runtime,
    pub manifest: String,
}

/// Finds service folders: direct sub-folders with a known manifest, or the root if none have one.
pub fn discover(project: &dyn Project) -> Vec<Discovered> {
    let found: Vec<Discovered> = project
        .subdirs(".")
        .into_iter()
        .filter(|d| !d.starts_with('.') && !IGNORED_DIRS.contains(&d.as_str()))
        .filter_map(|dir| {
            let (runtime, manifest) = detect_runtime(project, &dir)?;
            Some(Discovered { dir, runtime, manifest })
        })
        .collect();
    if !found.is_empty() {
        return found;
    }
    detect_runtime(project, ".")
        .map(|(runtime, manifest)| Discovered { dir: ".".to_owned(), runtime, manifest })
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_round_trips() {
        for text in ["node22", "python3.13", "go1.23", "rust", "rust1.80"] {
            assert_eq!(Runtime::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Runtime::parse("ruby3"), None);
        assert_eq!(Runtime::parse("node"), None);
    }

    #[test]
    fn detects_versions_from_manifests() {
        let project = MemProject::new()
            .with_file("api/package.json", r#"{"engines": {"node": ">=20.1"}}"#)
            .with_file("worker/pyproject.toml", "[project]\nrequires-python = \">=3.12\"\n")
            .with_file("gw/go.mod", "module x\n\ngo 1.22.4\n")
            .with_file("core/Cargo.toml", "[package]\nname = \"core\"\n")
            .with_file("web/package.json", "{}");
        let detected = |dir| detect_runtime(&project, dir).map(|(r, m)| (r.to_string(), m));
        assert_eq!(detected("api"), Some(("node20".into(), "api/package.json".into())));
        assert_eq!(detected("worker"), Some(("python3.12".into(), "worker/pyproject.toml".into())));
        assert_eq!(detected("gw"), Some(("go1.22".into(), "gw/go.mod".into())));
        assert_eq!(detected("core"), Some(("rust".into(), "core/Cargo.toml".into())));
        assert_eq!(detected("web").unwrap().0, "node22");
        assert_eq!(detected("missing"), None);
    }

    #[test]
    fn discovers_service_folders() {
        let project = MemProject::new()
            .with_file("api/package.json", "{}")
            .with_file("node_modules/x/package.json", "{}")
            .with_file("docs/readme.md", "")
            .with_file("worker/pyproject.toml", "");
        let dirs: Vec<_> = discover(&project).into_iter().map(|d| d.dir).collect();
        assert_eq!(dirs, ["api", "worker"]);
    }

    #[test]
    fn discovers_a_root_service() {
        let project = MemProject::new().with_file("go.mod", "go 1.23\n");
        let found = discover(&project);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].dir, ".");
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize_path("./api/"), "api");
        assert_eq!(normalize_path(".\\svc\\api"), "svc/api");
        assert_eq!(normalize_path("."), ".");
        assert_eq!(normalize_path("./"), ".");
    }
}
