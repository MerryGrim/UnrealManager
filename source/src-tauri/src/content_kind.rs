//! Resolve install behaviour from a selected package's file layout.
//! This is a bounded in-memory inspection of manifest/record paths, not a scan.

fn at_root(paths: &[String], prefix: &str) -> Option<&'static str> {
    let relative: Vec<&str> = paths.iter().filter_map(|path| path.strip_prefix(prefix)).collect();
    let descriptors = |extension: &str| relative.iter().filter(|path| !path.contains('/') && path.len() > extension.len() && path.ends_with(extension)).count();
    let projects = descriptors(".uproject");
    let plugins = descriptors(".uplugin");
    let project_content = relative.iter().any(|path| path.starts_with("content/") || path.starts_with("config/"));
    match (projects, plugins) {
        (1, 0) if project_content => Some("Project"),
        (0, 1) => Some("Plugin"),
        _ => None,
    }
}

pub(crate) fn from_paths<I, S>(declared: &str, paths: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    // A known plugin may ship an example project. A known project with missing
    // files must retain its stricter creation checks, not become Content-only.
    if declared.eq_ignore_ascii_case("Plugin") { return "Plugin".into(); }
    if declared.eq_ignore_ascii_case("Project") { return "Project".into(); }

    let mut names = Vec::new();
    for path in paths {
        let path = path.as_ref().replace('\\', "/");
        if path.is_empty() || path.len() > 2000 || path.chars().any(char::is_control)
            || path.split('/').any(|part| part.is_empty() || matches!(part, "." | "..") || part.contains(':')) {
            return "Asset".into();
        }
        names.push(path.to_ascii_lowercase());
    }
    if let Some(kind) = at_root(&names, "") { return kind.into(); }
    // Some manifests retain one outer package directory. A nested sample
    // beside other package files is deliberately not treated as the package.
    if let Some(first) = names.first().and_then(|name| name.split_once('/')).map(|(name, _)| format!("{name}/")) {
        if names.iter().all(|name| name.starts_with(&first)) {
            if let Some(kind) = at_root(&names, &first) { return kind.into(); }
        }
    }
    "Asset".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_complete_project_overrides_generic_asset_catalogue() {
        assert_eq!(from_paths("Asset", ["DMD_MSS.uproject", "Config/DefaultEngine.ini", "Content/MSS/System.uasset"]), "Project");
    }
    #[test]
    fn common_outer_project_directory_is_supported() {
        assert_eq!(from_paths("Asset", ["MSS/DMD_MSS.uproject", "MSS/Config/DefaultGame.ini", "MSS/Content/MSS.uasset"]), "Project");
    }
    #[test]
    fn project_with_bundled_plugins_remains_project() {
        assert_eq!(from_paths("Asset", ["Game.uproject", "Content/Map.umap", "Plugins/Helper/Helper.uplugin"]), "Project");
    }
    #[test]
    fn explicit_plugin_never_becomes_its_example_project() {
        assert_eq!(from_paths("Plugin", ["Demo.uproject", "Content/Example.uasset", "Example.uplugin"]), "Plugin");
        assert_eq!(from_paths("Plugin", ["Example/Example.uplugin", "Samples/Game/Game.uproject", "Samples/Game/Content/Map.umap"]), "Plugin");
    }
    #[test]
    fn explicit_project_keeps_strict_creation_path_even_for_missing_files() {
        assert_eq!(from_paths("Project", ["Original.uproject"]), "Project");
        assert_eq!(from_paths("Project", ["Content/Only.uasset"]), "Project");
    }
    #[test]
    fn content_only_and_nested_samples_are_not_promoted() {
        assert_eq!(from_paths("Asset", ["Content/Pack/Mesh.uasset"]), "Asset");
        assert_eq!(from_paths("Asset", ["Content/Pack/Mesh.uasset", "Samples/Game/Game.uproject", "Samples/Game/Config/Game.ini"]), "Asset");
        assert_eq!(from_paths("Asset", ["README.txt", "Sample/Game.uproject", "Sample/Content/Map.umap"]), "Asset");
    }
    #[test]
    fn descriptor_without_project_structure_is_not_a_new_project() {
        assert_eq!(from_paths("Asset", ["Only.uproject"]), "Asset");
        assert_eq!(from_paths("Asset", [".uproject", "Content/Map.umap"]), "Asset");
    }
    #[test]
    fn ambiguous_root_descriptors_are_not_automatically_selected() {
        assert_eq!(from_paths("Asset", ["A.uproject", "B.uproject", "Content/Map.umap"]), "Asset");
        assert_eq!(from_paths("Asset", ["A.uproject", "A.uplugin", "Content/Map.umap"]), "Asset");
    }
    #[test]
    fn root_and_single_wrapped_plugins_are_detected() {
        assert_eq!(from_paths("Asset", ["Tools.uplugin", "Source/Tools/Tools.Build.cs"]), "Plugin");
        assert_eq!(from_paths("Asset", ["Tools/Tools.uplugin", "Tools/Content/Icon.uasset"]), "Plugin");
        assert_eq!(from_paths("Asset", ["Content/Pack/Example.uplugin", "README.txt"]), "Asset");
    }
    #[test]
    fn case_and_windows_separators_match_unreal_package_paths() {
        assert_eq!(from_paths("Asset", ["PACK\\GAME.UPROJECT", "PACK\\CONFIG\\DefaultGame.ini"]), "Project");
    }
    #[test]
    fn malformed_paths_do_not_supply_a_type_hint() {
        for path in ["../Game.uproject", "/Game.uproject", "C:/Game.uproject", "Game\0.uproject", "Game.uproject/"] {
            assert_eq!(from_paths("Asset", [path, "Config/DefaultGame.ini"]), "Asset");
        }
        assert_eq!(from_paths("Asset", std::iter::empty::<&str>()), "Asset");
    }
}
