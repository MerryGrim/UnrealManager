//! Local, read-only discovery. Deliberately bounded: never recursively scan a drive.
use crate::EngineInstall;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_VERSION_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES_PER_DIRECTORY: usize = 512;
const MAX_SEARCH_ENTRIES: usize = 8192;

fn read_json(path: &Path, limit: u64) -> Option<serde_json::Value> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Keep the branch (major.minor) used by the component/Fab compatibility API.
/// The full patch/changelist is read independently by the engine catalogue.
fn installed_version(path: &Path) -> Option<(u32, u32, u32)> {
    let bin = path.join("Engine/Binaries/Win64");
    if !(bin.join("UnrealEditor.exe").is_file() || bin.join("UE4Editor.exe").is_file()) {
        return None;
    }
    let value = read_json(&path.join("Engine/Build/Build.version"), MAX_VERSION_BYTES)?;
    let part = |key: &str| u32::try_from(value[key].as_u64()?).ok();
    let version = (
        part("MajorVersion")?,
        part("MinorVersion")?,
        part("PatchVersion")?,
    );
    (version.0 > 0).then_some(version)
}

fn epic_paths(program_data: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(data) = read_json(
        &program_data.join("Epic/UnrealEngineLauncher/LauncherInstalled.dat"),
        8 * 1024 * 1024,
    ) {
        out.extend(
            data["InstallationList"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["InstallLocation"].as_str().map(PathBuf::from)),
        );
    }
    // Some restored Launcher installs only have the per-product .item metadata.
    if let Ok(entries) = fs::read_dir(program_data.join("Epic/EpicGamesLauncher/Data/Manifests")) {
        for entry in entries.take(MAX_SEARCH_ENTRIES).flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("item"))
            {
                continue;
            }
            let Some(item) = read_json(&path, 1024 * 1024) else {
                continue;
            };
            if item["CatalogNamespace"] == "ue"
                && item["AppName"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("UE_"))
            {
                if let Some(path) = item["InstallLocation"].as_str() {
                    out.push(PathBuf::from(path));
                }
            }
        }
    }
    out
}

#[cfg(windows)]
fn registry_paths() -> Vec<PathBuf> {
    use winreg::{enums::*, RegKey};
    let mut out = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for name in [
        r"SOFTWARE\EpicGames\Unreal Engine",
        r"SOFTWARE\WOW6432Node\EpicGames\Unreal Engine",
    ] {
        if let Ok(key) = hklm.open_subkey(name) {
            for version in key.enum_keys().take(MAX_ENTRIES_PER_DIRECTORY).flatten() {
                if let Ok(sub) = key.open_subkey(version) {
                    if let Ok(path) = sub.get_value::<String, _>("InstalledDirectory") {
                        out.push(PathBuf::from(path));
                    }
                }
            }
        }
    }
    if let Ok(key) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Epic Games\Unreal Engine\Builds")
    {
        for (name, _) in key.enum_values().take(MAX_ENTRIES_PER_DIRECTORY).flatten() {
            if let Ok(path) = key.get_value::<String, _>(name) {
                out.push(PathBuf::from(path));
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn local_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    // DRIVE_FIXED from GetDriveTypeW; no WindowsProgramming dependency is needed.
    const FIXED_DRIVE_TYPE: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter_map(|index| {
            if mask & (1 << index) == 0 {
                return None;
            }
            let name = format!("{}:\\", char::from(b'A' + index));
            let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
            (unsafe { GetDriveTypeW(wide.as_ptr()) } == FIXED_DRIVE_TYPE)
                .then(|| PathBuf::from(name))
        })
        .collect()
}

#[cfg(not(windows))]
fn local_drives() -> Vec<PathBuf> {
    Vec::new()
}

fn known_container(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "epic games" | "unreal" | "unreal engine" | "unrealengine" | "ue" | "engines" | "games"
    )
}

fn search_roots(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    // Try specific common containers first so a large drive root cannot hide them.
    for drive in drives {
        for relative in [
            "Program Files/Epic Games",
            "Program Files/Unreal Engine",
            "Epic Games",
            "Unreal",
            "Unreal Engine",
            "UnrealEngine",
            "UE",
            "Engines",
            "Games",
        ] {
            roots.push(drive.join(relative));
        }
    }
    for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(path) = std::env::var_os(key) {
            roots.push(PathBuf::from(path).join("Epic Games"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        for name in ["UnrealEngine", "Unreal Engine", "Unreal", "Epic Games"] {
            roots.push(home.join(name));
        }
    }
    roots.extend_from_slice(drives);
    roots
}

fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn discover(known: Vec<PathBuf>, roots: Vec<PathBuf>) -> Vec<EngineInstall> {
    let mut candidates = known;
    let mut visited = BTreeSet::new();
    let mut remaining = MAX_SEARCH_ENTRIES;
    fn visit(
        root: &Path,
        depth: usize,
        candidates: &mut Vec<PathBuf>,
        visited: &mut BTreeSet<String>,
        remaining: &mut usize,
    ) {
        if !visited.insert(path_key(root)) {
            return;
        }
        if installed_version(root).is_some() {
            candidates.push(root.to_path_buf());
            return;
        }
        if *remaining == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.take(MAX_ENTRIES_PER_DIRECTORY).flatten() {
            if *remaining == 0 {
                break;
            }
            *remaining -= 1;
            // Do not follow junctions during heuristic search; explicit registry/index paths
            // are validated separately and can still name a linked installation.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if installed_version(&path).is_some() {
                candidates.push(path);
            } else if depth < 2 && known_container(&entry.file_name().to_string_lossy()) {
                visit(&path, depth + 1, candidates, visited, remaining);
            }
        }
    }
    for root in roots {
        visit(&root, 0, &mut candidates, &mut visited, &mut remaining);
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for path in candidates {
        if path.join(".um-engine-update.json").exists() {
            continue;
        }
        let Some((major, minor, patch)) = installed_version(&path) else {
            continue;
        };
        if !seen.insert(path_key(&path)) {
            continue;
        }
        result.push((
            (major, minor, patch),
            EngineInstall {
                name: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unreal Engine")
                    .into(),
                version: format!("{major}.{minor}"),
                path: path.to_string_lossy().into_owned(),
                source: "Local installation".into(),
            },
        ));
    }
    result.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    result.into_iter().map(|(_, item)| item).collect()
}

pub(crate) fn common_roots() -> Vec<PathBuf> {
    search_roots(&local_drives())
}

pub(crate) fn scan() -> Vec<EngineInstall> {
    let program_data = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let mut known = registry_paths();
    known.extend(epic_paths(&program_data));
    known.extend(crate::engine::managed_roots());
    discover(known, common_roots())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("um-discovery-{}", uuid::Uuid::new_v4())))
        }
        fn engine(&self, relative: &str, version: (u32, u32, u32), legacy: bool) -> PathBuf {
            let root = self.0.join(relative);
            fs::create_dir_all(root.join("Engine/Build")).unwrap();
            fs::create_dir_all(root.join("Engine/Binaries/Win64")).unwrap();
            fs::write(
                root.join("Engine/Binaries/Win64").join(if legacy {
                    "UE4Editor.exe"
                } else {
                    "UnrealEditor.exe"
                }),
                b"fixture",
            )
            .unwrap();
            fs::write(root.join("Engine/Build/Build.version"), serde_json::to_vec(&serde_json::json!({"MajorVersion":version.0,"MinorVersion":version.1,"PatchVersion":version.2,"Changelist":123})).unwrap()).unwrap();
            root
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_restored_engines_without_epic_metadata_and_uses_real_versions() {
        let fixture = Fixture::new();
        fixture.engine("Unreal/renamed-installation", (5, 8, 3), false);
        fixture.engine("Games/Epic Games/UE_5.10", (5, 10, 1), false);
        fixture.engine("UE_4.27", (4, 27, 2), true);
        let found = discover(Vec::new(), vec![fixture.0.clone()]);
        assert_eq!(
            found.iter().map(|e| e.version.as_str()).collect::<Vec<_>>(),
            ["5.10", "5.8", "4.27"]
        );
    }

    #[test]
    fn rejects_missing_invalid_or_oversized_build_version_and_pending_installation() {
        let fixture = Fixture::new();
        let invalid = fixture.engine("invalid", (5, 8, 3), false);
        fs::write(invalid.join("Engine/Build/Build.version"), b"{}").unwrap();
        let missing = fixture.engine("missing", (5, 8, 3), false);
        fs::remove_file(missing.join("Engine/Build/Build.version")).unwrap();
        let oversized = fixture.engine("oversized", (5, 8, 3), false);
        fs::write(
            oversized.join("Engine/Build/Build.version"),
            vec![b' '; MAX_VERSION_BYTES as usize + 1],
        )
        .unwrap();
        let pending = fixture.engine("pending", (5, 8, 3), false);
        fs::write(pending.join(".um-engine-update.json"), b"{}").unwrap();
        assert!(discover(Vec::new(), vec![fixture.0.clone()]).is_empty());
    }

    #[test]
    fn does_not_recurse_into_arbitrary_user_directories() {
        let fixture = Fixture::new();
        let root = fixture.engine("UserFiles/deep-install", (5, 8, 3), false);
        assert!(discover(Vec::new(), vec![fixture.0.clone()]).is_empty());
        assert_eq!(
            discover(vec![root.clone(), root.join(".")], vec![root]).len(),
            1
        );
    }

    #[test]
    fn discovers_item_metadata_and_deduplicates_launcher_paths() {
        let fixture = Fixture::new();
        let root = fixture.engine("CustomLocation", (5, 8, 3), false);
        let base = fixture.0.join("ProgramData");
        let item_dir = base.join("Epic/EpicGamesLauncher/Data/Manifests");
        fs::create_dir_all(&item_dir).unwrap();
        fs::create_dir_all(base.join("Epic/UnrealEngineLauncher")).unwrap();
        fs::write(item_dir.join("engine.item"), serde_json::to_vec(&serde_json::json!({"CatalogNamespace":"ue","AppName":"UE_5.8","InstallLocation":root})).unwrap()).unwrap();
        fs::write(
            base.join("Epic/UnrealEngineLauncher/LauncherInstalled.dat"),
            serde_json::to_vec(&serde_json::json!({"InstallationList":[{"InstallLocation":root}]}))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(epic_paths(&base).len(), 2);
        assert_eq!(discover(epic_paths(&base), Vec::new()).len(), 1);
    }
}
