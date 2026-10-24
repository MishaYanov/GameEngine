fn main() {
	let _logging =
		engine_logger::init_application("compiler").expect("failed to initialize compiler logger");
	let args: Vec<String> = std::env::args().skip(1).collect();
	if args.len() != 2 {
		eprintln!("Usage: compiler <project-name> <destination-folder>");
		std::process::exit(2);
	}
	engine_logger::info!(target: "compiler", "Project build/install started");
	match compiler::install_project(&args[0], &args[1]) {
		Ok(path) => {
			engine_logger::info!(target: "compiler", "Project build/install completed");
			println!("Project installed at {}", path.display());
		}
		Err(error) => {
			engine_logger::error!(target: "compiler", "Project build/install failed: operation=install_project");
			eprintln!("{error}");
			drop(_logging); // process::exit does not run the logging guard.
			std::process::exit(1);
		}
	}
}
