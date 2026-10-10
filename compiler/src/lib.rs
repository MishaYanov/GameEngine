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
        fs::create_dir_all(path.join("assets/textures"))?;
        for texture in ["crate.png", "bricks.jpg"] {
            fs::copy(
                workspace.join("apps/game/assets/textures").join(texture),
                path.join("assets/textures").join(texture),
            )?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
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
