use std::{
	env,
	path::{Path, PathBuf},
	process::Command,
};

fn main() {
	println!("cargo:rerun-if-changed=shaders/triangle.vert");
	println!("cargo:rerun-if-changed=shaders/triangle.frag");

	compile_shader(
		"shaders/triangle.vert",
		"triangle.vert.spv",
	);

	compile_shader(
		"shaders/triangle.frag",
		"triangle.frag.spv",
	);
}

fn compile_shader(source: &str, output_name: &str) {
	let out_dir = PathBuf::from(
		env::var("OUT_DIR")
			.expect("OUT_DIR is not set"),
	);

	let output = out_dir.join(output_name);

	let glslc = find_glslc();

	let status = Command::new(&glslc)
		.arg(source)
		.arg("-o")
		.arg(&output)
		.status()
		.unwrap_or_else(|error| {
			panic!(
				"failed to execute glslc at {:?}: {}",
				glslc,
				error
			)
		});

	if !status.success() {
		panic!(
			"failed to compile shader: {}",
			source
		);
	}
}

fn find_glslc() -> PathBuf {
	if let Ok(vulkan_sdk) = env::var("VULKAN_SDK") {
		let executable = if cfg!(windows) {
			Path::new(&vulkan_sdk)
				.join("Bin")
				.join("glslc.exe")
		} else {
			Path::new(&vulkan_sdk)
				.join("bin")
				.join("glslc")
		};

		if executable.exists() {
			return executable;
		}
	}

	if cfg!(windows) {
		PathBuf::from("glslc.exe")
	} else {
		PathBuf::from("glslc")
	}
}