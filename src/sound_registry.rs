use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::alerts::managed_packs_dir;

const REGISTRY_URL: &str = "https://peonping.github.io/registry/index.json";
const GITHUB_RAW_ROOT: &str = "https://raw.githubusercontent.com/";
const USER_AGENT: &str = "omp-gtk/0.1";
const MAX_REGISTRY_BYTES: usize = 8 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_SOUND_BYTES: usize = 1024 * 1024;
const MAX_PACK_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegistryAuthor {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegistryPack {
    pub name: String,
    pub display_name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub author: RegistryAuthor,
    pub trust_tier: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub license: String,
    pub sound_count: u32,
    #[serde(default)]
    pub total_size_bytes: u64,
    pub source_repo: String,
    pub source_ref: String,
    pub source_path: String,
    #[serde(default)]
    pub manifest_sha256: String,
    #[serde(default)]
    pub quality: Option<String>,
}

impl RegistryPack {
    pub fn metadata(&self) -> String {
        let language = if self.language.trim().is_empty() {
            "Unknown language"
        } else {
            &self.language
        };
        let license = if self.license.trim().is_empty() {
            "License not listed"
        } else {
            &self.license
        };
        let size = if self.total_size_bytes == 0 {
            "Size unavailable".to_owned()
        } else {
            format_size(self.total_size_bytes)
        };
        format!(
            "v{} · {} · {} · {} sounds · {}",
            self.version, language, license, self.sound_count, size
        )
    }

    pub fn source_label(&self) -> String {
        let mut labels = vec![self.author.name.clone(), title_case(&self.trust_tier)];
        if let Some(quality) = self
            .quality
            .as_deref()
            .filter(|quality| !quality.is_empty())
        {
            labels.push(format!("{} quality", title_case(quality)));
        }
        labels.join(" · ")
    }
}

#[derive(Deserialize)]
struct RegistryIndex {
    packs: Vec<RegistryPack>,
}

#[derive(Deserialize)]
struct DownloadManifest {
    name: String,
    categories: HashMap<String, DownloadCategory>,
}

#[derive(Deserialize)]
struct DownloadCategory {
    sounds: Vec<DownloadSound>,
}

#[derive(Deserialize)]
struct DownloadSound {
    file: PathBuf,
    #[serde(default)]
    sha256: Option<String>,
}

pub(crate) fn fetch_registry() -> Result<Vec<RegistryPack>, String> {
    let mut index = read_json::<RegistryIndex>(REGISTRY_URL, MAX_REGISTRY_BYTES)?;
    index
        .packs
        .retain(|pack| validate_registry_entry(pack).is_ok());
    index.packs.sort_by(|left, right| {
        left.display_name
            .to_ascii_lowercase()
            .cmp(&right.display_name.to_ascii_lowercase())
            .then(left.name.cmp(&right.name))
    });
    Ok(index.packs)
}

pub(crate) fn install_pack(pack: &RegistryPack) -> Result<PathBuf, String> {
    validate_registry_entry(pack)?;
    let source_prefix = normalized_source_prefix(&pack.source_path)?;
    let manifest_repo_path = prefixed_repo_path(source_prefix.as_deref(), "openpeon.json");
    let (manifest_bytes, resolved_ref) = load_verified_manifest(pack, &manifest_repo_path)?;
    let manifest = serde_json::from_slice::<DownloadManifest>(&manifest_bytes)
        .map_err(|error| format!("The pack manifest is invalid: {error}"))?;
    if manifest.name != pack.name {
        return Err(format!(
            "Registry pack {} contains a manifest named {}",
            pack.name, manifest.name
        ));
    }

    let mut sounds = BTreeMap::<PathBuf, Option<String>>::new();
    for sound in manifest
        .categories
        .values()
        .flat_map(|category| &category.sounds)
    {
        let path = normalized_sound_path(&sound.file)?;
        match sounds.entry(path) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(sound.sha256.clone());
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() != &sound.sha256 => {
                return Err("The manifest gives one sound conflicting checksums".to_owned());
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }
    if sounds.is_empty() {
        return Err("The pack does not contain any sounds".to_owned());
    }

    let packs_dir = managed_packs_dir();
    fs::create_dir_all(&packs_dir)
        .map_err(|error| format!("Could not create {}: {error}", packs_dir.display()))?;
    let destination = packs_dir.join(&pack.name);
    if destination.exists() {
        return Err(format!("{} is already installed", pack.display_name));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = packs_dir.join(format!(
        ".{}.install-{}-{nonce}",
        pack.name,
        std::process::id()
    ));
    fs::create_dir(&temporary)
        .map_err(|error| format!("Could not prepare pack installation: {error}"))?;

    let result = (|| {
        fs::write(temporary.join("openpeon.json"), &manifest_bytes)
            .map_err(|error| format!("Could not write pack manifest: {error}"))?;
        let mut downloaded = manifest_bytes.len();
        for (relative_path, checksum) in sounds {
            let repo_path =
                prefixed_repo_path(source_prefix.as_deref(), &relative_path.to_string_lossy());
            let url = github_raw_url_for_ref(pack, &resolved_ref, &repo_path)?;
            let bytes = read_bytes(url.as_str(), MAX_SOUND_BYTES)?;
            validate_audio(&relative_path, &bytes)?;
            if let Some(checksum) = checksum.as_deref() {
                verify_sha256(&bytes, checksum, &relative_path.to_string_lossy())?;
            }
            downloaded = downloaded.saturating_add(bytes.len());
            if downloaded > MAX_PACK_BYTES {
                return Err("The downloaded pack exceeds the 50 MB size limit".to_owned());
            }
            let path = temporary.join(&relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
            }
            fs::write(&path, bytes)
                .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
        }
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("Could not finish pack installation: {error}"))?;
        Ok(destination.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn validate_registry_entry(pack: &RegistryPack) -> Result<(), String> {
    let valid_name = !pack.name.is_empty()
        && pack.name.len() <= 64
        && pack.name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
        });
    if !valid_name {
        return Err("The registry pack name is invalid".to_owned());
    }
    let mut repository = pack.source_repo.split('/');
    if repository.next().is_none()
        || repository.next().is_none()
        || repository.next().is_some()
        || pack.source_ref.trim().is_empty()
    {
        return Err("The registry source repository is invalid".to_owned());
    }
    if pack.total_size_bytes > MAX_PACK_BYTES as u64 {
        return Err("The registry reports a pack larger than 50 MB".to_owned());
    }
    if !is_sha256(&pack.manifest_sha256) {
        return Err("The registry manifest checksum is invalid".to_owned());
    }
    Ok(())
}

fn normalized_source_prefix(path: &str) -> Result<Option<PathBuf>, String> {
    if path == "." || path.is_empty() {
        return Ok(None);
    }
    let path = Path::new(path);
    if !safe_relative_path(path) {
        return Err("The registry source path is unsafe".to_owned());
    }
    Ok(Some(path.to_owned()))
}

fn normalized_sound_path(path: &Path) -> Result<PathBuf, String> {
    if !safe_relative_path(path) {
        return Err(format!("Unsafe sound path: {}", path.display()));
    }
    let path = if path.components().count() == 1 {
        Path::new("sounds").join(path)
    } else {
        path.to_owned()
    };
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "wav" | "mp3" | "ogg") {
        return Err(format!("Unsupported sound format: {}", path.display()));
    }
    Ok(path)
}

fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn prefixed_repo_path(prefix: Option<&Path>, relative: &str) -> String {
    match prefix {
        Some(prefix) => format!("{}/{}", prefix.to_string_lossy(), relative),
        None => relative.to_owned(),
    }
}

#[cfg(test)]
fn github_raw_url(pack: &RegistryPack, path: &str) -> Result<Url, String> {
    github_raw_url_for_ref(pack, &pack.source_ref, path)
}

fn load_verified_manifest(
    pack: &RegistryPack,
    manifest_path: &str,
) -> Result<(Vec<u8>, String), String> {
    let mut last_error = None;
    for (index, source_ref) in [pack.source_ref.as_str(), "main", "master"]
        .into_iter()
        .enumerate()
    {
        if index > 0 && source_ref == pack.source_ref {
            continue;
        }
        let url = github_raw_url_for_ref(pack, source_ref, manifest_path)?;
        match read_bytes(url.as_str(), MAX_MANIFEST_BYTES).and_then(|bytes| {
            verify_sha256(&bytes, &pack.manifest_sha256, "pack manifest")?;
            Ok(bytes)
        }) {
            Ok(bytes) => return Ok((bytes, source_ref.to_owned())),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "Could not download the pack manifest".to_owned()))
}

fn github_raw_url_for_ref(
    pack: &RegistryPack,
    source_ref: &str,
    path: &str,
) -> Result<Url, String> {
    let (owner, repository) = split_repository(&pack.source_repo)?;
    let mut segments = vec![owner, repository, source_ref];
    segments.extend(path.split('/'));
    github_url(GITHUB_RAW_ROOT, &segments, None)
}

fn split_repository(repository: &str) -> Result<(&str, &str), String> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| "The registry source repository is invalid".to_owned())?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err("The registry source repository is invalid".to_owned());
    }
    Ok((owner, name))
}

fn github_url(root: &str, segments: &[&str], query: Option<(&str, &str)>) -> Result<Url, String> {
    let mut url = Url::parse(root).map_err(|error| format!("Invalid GitHub URL: {error}"))?;
    url.path_segments_mut()
        .map_err(|_| "Invalid GitHub URL root".to_owned())?
        .extend(segments.iter().copied());
    if let Some((key, value)) = query {
        url.query_pairs_mut().append_pair(key, value);
    }
    Ok(url)
}

fn read_json<T: for<'de> Deserialize<'de>>(url: &str, limit: usize) -> Result<T, String> {
    let bytes = read_bytes(url, limit)?;
    serde_json::from_slice(&bytes).map_err(|error| format!("Invalid response from {url}: {error}"))
}

fn read_bytes(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Could not download {url}: {error}"))?;
    response
        .body_mut()
        .with_config()
        .limit(limit as u64)
        .read_to_vec()
        .map_err(|error| format!("Could not read {url}: {error}"))
}

fn verify_sha256(bytes: &[u8], expected: &str, description: &str) -> Result<(), String> {
    if !is_sha256(expected) {
        return Err(format!("The {description} checksum is invalid"));
    }
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("The {description} checksum did not match"))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_audio(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid = match extension.as_str() {
        "wav" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "ogg" => bytes.starts_with(b"OggS"),
        "mp3" => {
            bytes.starts_with(b"ID3")
                || bytes
                    .get(0..2)
                    .is_some_and(|header| header[0] == 0xff && header[1] & 0xe0 == 0xe0)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{} is not valid audio", path.display()))
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> RegistryPack {
        RegistryPack {
            name: "test-pack".to_owned(),
            display_name: "Test Pack".to_owned(),
            version: "1.0.0".to_owned(),
            description: "Test sounds".to_owned(),
            author: RegistryAuthor {
                name: "Tester".to_owned(),
            },
            trust_tier: "community".to_owned(),
            categories: vec!["task.complete".to_owned()],
            language: "en".to_owned(),
            license: "CC0".to_owned(),
            sound_count: 1,
            total_size_bytes: 1024,
            source_repo: "example/sounds".to_owned(),
            source_ref: "v1.0.0".to_owned(),
            source_path: "test-pack".to_owned(),
            manifest_sha256: "a".repeat(64),
            quality: Some("gold".to_owned()),
        }
    }

    #[test]
    fn builds_encoded_source_urls() {
        let pack = pack();
        assert_eq!(
            github_raw_url(&pack, "test-pack/sounds/done sound.ogg")
                .expect("raw URL")
                .as_str(),
            "https://raw.githubusercontent.com/example/sounds/v1.0.0/test-pack/sounds/done%20sound.ogg"
        );
    }

    #[test]
    fn normalizes_manifest_sound_paths_without_allowing_traversal() {
        assert_eq!(
            normalized_sound_path(Path::new("done.ogg")).expect("short path"),
            PathBuf::from("sounds/done.ogg")
        );
        assert!(normalized_sound_path(Path::new("../done.ogg")).is_err());
        assert!(normalized_sound_path(Path::new("sounds/readme.txt")).is_err());
    }

    #[test]
    fn validates_supported_audio_headers() {
        assert!(validate_audio(Path::new("sound.ogg"), b"OggSdata").is_ok());
        assert!(validate_audio(Path::new("sound.wav"), b"RIFF0000WAVEdata").is_ok());
        assert!(validate_audio(Path::new("sound.mp3"), b"ID3data").is_ok());
        assert!(validate_audio(Path::new("sound.ogg"), b"not audio").is_err());
    }

    #[test]
    fn rejects_oversized_or_untrusted_registry_metadata() {
        let mut pack = pack();
        assert!(validate_registry_entry(&pack).is_ok());
        pack.name = "../escape".to_owned();
        assert!(validate_registry_entry(&pack).is_err());
    }
}
