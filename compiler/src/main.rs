fn main() {
	let _logging =
		engine_logger::init_application("compiler").expect("failed to initialize compiler logger");
	let args: Vec<String> = std::env::args().skip(1).collect();
	let stage = args.first().is_some_and(|argument| argument == "--stage");
	let export = args.first().is_some_and(|argument| argument == "--export");
	if !matches!(args.as_slice(), [name, _] if !name.starts_with("--"))
		&& !((stage || export) && args.len() == 3)
	{
		eprintln!(
			"Usage: compiler <project-name> <destination-folder>\n       compiler --stage <project.json> <destination-folder>\n       compiler --export <project.json> <destination-folder>"
		);
		std::process::exit(2);
	}
	engine_logger::info!(target: "compiler", "Project build/install started");
	let result = if stage {
		stage_manifest(std::path::Path::new(&args[1]), &args[2])
	} else if export {
		compiler::install_project_from_manifest(std::path::Path::new(&args[1]), &args[2])
	} else {
		compiler::install_project(&args[0], &args[1])
	};
	match result {
		Ok(path) => {
			engine_logger::info!(target: "compiler", "Project build/install completed");
			let operation = if stage { "staged" } else { "installed" };
			println!("Project {operation} at {}", path.display());
		}
		Err(error) => {
			engine_logger::error!(target: "compiler", "Project build/install failed: operation=install_project");
			eprintln!("{error}");
			drop(_logging); // process::exit does not run the logging guard.
			std::process::exit(1);
		}
	}
}

fn stage_manifest(
	manifest: &std::path::Path,
	destination: &str,
) -> Result<std::path::PathBuf, String> {
	let project = engine_core::project::ProjectData::load(manifest)?;
	let assets = manifest
		.parent()
		.ok_or("Project manifest has no parent folder")?
		.join("assets");
	let prepared = compiler::prepare_project(&project, &assets)?;
	let staged = compiler::stage_prepared_project(&prepared, destination)?;
	Ok(staged.path().to_path_buf())
}
