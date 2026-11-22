use super::{
	Options,
	source::{Source, source},
};
use engine_core::{
	Engine, EngineBuilder,
	command::{EditorCommandDto, EditorCommandRequest},
};
use std::{sync::Arc, time::Instant};

#[test]
#[ignore = "local CPU benchmark; run in release with --ignored --nocapture"]
fn measure_camera_room_fixed_steps() {
	let fixture = test_project::ProjectFixture::new("camera-room");
	let options = Options {
		project: Some(fixture.manifest_path()),
		..Default::default()
	};
	let Source {
		project,
		parsed,
		documents,
		asset_root,
		..
	} = source(&options).unwrap();
	let step = 1.0 / f64::from(project.fixed_hz);
	let mut engine = EngineBuilder::new(parsed.data, project)
		.with_scenes(documents)
		.module_resolver(Arc::new(gameplay_catalog::resolve))
		.animation_resolver(Arc::new(move |scene| {
			bridge::animation_asset::prepare_scene(&asset_root, scene)
		}))
		.build()
		.unwrap();
	let execute = |engine: &mut Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap()
	};
	execute(&mut engine, EditorCommandDto::StartPlay);
	execute(
		&mut engine,
		EditorCommandDto::SetCharacterInput {
			key: "player".into(),
			movement_xz: [0.0, 1.0],
			jump_held: false,
		},
	);
	let mut samples = Vec::new();
	let mut blocked_steps = 0;
	let mut min_x = f32::MAX;
	let mut max_x = f32::MIN;
	let mut direction = 0.0_f32;
	for frame in 0..720 {
		// Walk toward the rear wall for 72 warmup steps, then move laterally along it.
		let x = engine
			.session()
			.orbit_state()
			.unwrap()
			.previous_target_pivot[0];
		if frame == 72 || (direction != 0.0 && x * direction > 0.35) {
			direction = if direction == 0.0 { 1.0 } else { -direction };
			execute(
				&mut engine,
				EditorCommandDto::SetCharacterInput {
					key: "player".into(),
					movement_xz: [direction, 0.0],
					jump_held: false,
				},
			);
		}
		let start = Instant::now();
		let outcome = engine.tick(step).unwrap();
		let micros = start.elapsed().as_secs_f64() * 1.0e6;
		assert_eq!(outcome.steps, 1);
		if frame >= 120 {
			samples.push(micros);
			let state = engine.session().orbit_state().unwrap();
			blocked_steps += usize::from(state.obstruction.is_some());
			max_x = max_x.max(state.filtered_pivot[0]);
			min_x = min_x.min(state.filtered_pivot[0]);
		}
	}
	assert!(max_x - min_x > 0.1);
	assert_eq!(blocked_steps, 600, "pivot X range {min_x}..{max_x}");
	samples.sort_by(f64::total_cmp);
	println!(
		"camera-room 600 fixed steps after 120 warmups at 60 Hz, blocked={blocked_steps} pivot_x=[{min_x:.3},{max_x:.3}] step_us p50={:.3} p95={:.3} max={:.3}",
		samples[299], samples[569], samples[599]
	);
}

#[path = "../../../../tests/support/project_fixture.rs"]
mod test_project;
