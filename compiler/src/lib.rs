use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn project_path(name: &str, destination: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 80
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ')
        || name.ends_with(' ')
    {
        return Err(
            "Use a project name of 1–80 letters, numbers, spaces, hyphens or underscores.".into(),
        );
    }
    let upper = name.to_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&upper.as_str())
    {
        return Err("Choose a name that is not a reserved Windows filename.".into());
    }
    let destination = Path::new(destination.trim());
    if !destination.is_absolute() || !destination.is_dir() {
        return Err("Destination must be an existing absolute folder path.".into());
    }
    let path = destination.join(name);
    if path.exists() {
        return Err("A folder with this project name already exists. Choose another name.".into());
    }
    Ok(path)
}

/// Build the engine stub, then install it into a newly reserved project folder.
pub fn install_project(name: &str, destination: &str) -> Result<PathBuf, String> {
    let path = project_path(name, destination)?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Missing engine workspace")?;
    let target = workspace.join("target");
    let output = Command::new("cargo")
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "--package",
            "game",
            "--release",
            "--target-dir",
        ])
        .arg(&target)
        .output()
        .map_err(|e| format!("Could not start Cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Game build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // create_dir is atomic: never merge into an existing project.
    fs::create_dir(&path).map_err(|e| format!("Could not create project folder: {e}"))?;
    let result = (|| -> Result<(), std::io::Error> {
        fs::copy(
            target
                .join("release")
                .join(format!("game{}", std::env::consts::EXE_SUFFIX)),
            path.join(format!("game{}", std::env::consts::EXE_SUFFIX)),
        )?;
        copy_assets(&workspace.join("apps/game/assets"), &path.join("assets"))?;
        fs::write(path.join("project-name.txt"), name.trim())?;
        fs::write(
            path.join("README.txt"),
            "Triangle starter project\nRun game.exe to open the Vulkan graphics window.\nRequires a Vulkan-capable GPU and driver.\nThis is a runtime stub; scene editing and project compilation come later.\n",
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        // Only remove the folder this invocation just created.
        let _ = fs::remove_dir_all(&path);
        return Err(format!("Could not install project: {error}"));
    }
    Ok(path)
}

fn copy_assets(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let output = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_assets(&entry.path(), &output)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), output)?;
        } else {
            return Err(std::io::Error::other("Unsupported asset file type"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_names_enforce_length_and_normalize_surrounding_whitespace() {
        let root = env!("CARGO_MANIFEST_DIR");
        assert!(project_path(&"a".repeat(81), root).is_err());
        assert!(project_path(&"a".repeat(80), root).is_ok());
        assert_eq!(
            project_path("  Demo QA  ", root).unwrap(),
            Path::new(root).join("Demo QA")
        );
        assert!(project_path("Demo", "").is_err());
    }
    #[test]
    fn installation_copies_models_and_nested_dependencies() {
        let destination =
            std::env::temp_dir().join(format!("engine-assets-{}", std::process::id()));
        fs::create_dir(&destination).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("apps/game/assets");
        let result = copy_assets(&source, &destination);
        let model = fs::read(destination.join("props/robot.glb"));
        let texture = fs::read(destination.join("textures/crate.png"));
        fs::remove_dir_all(&destination).unwrap();
        result.unwrap();
        assert_eq!(
            model.unwrap(),
            fs::read(source.join("props/robot.glb")).unwrap()
        );
        assert_eq!(
            texture.unwrap(),
            fs::read(source.join("textures/crate.png")).unwrap()
        );
    }
    #[test]
    fn rejects_unsafe_names_and_destinations() {
        for name in ["", "../escape", "a/b", "CON", "Lpt1", "a:stream"] {
            assert!(project_path(name, env!("CARGO_MANIFEST_DIR")).is_err());
        }
        assert!(project_path("Demo", "relative").is_err());
    }
    #[test]
    fn resolves_new_project_and_rejects_existing_folder() {
        let root = env!("CARGO_MANIFEST_DIR");
        assert_eq!(
            project_path("Triangle Demo", root).unwrap(),
            Path::new(root).join("Triangle Demo")
        );
        assert!(project_path("src", root).is_err());
    }
}
