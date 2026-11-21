use bridge::animation_asset;
use engine_core::{
	EntityKey, SceneData,
	animation::*,
	asset_catalog::AssetId,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::CameraData,
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};
use std::{
	collections::BTreeMap,
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};
const GLB: &[u8] = include_bytes!("../../../apps/game/assets/characters/two-joint.glb");
fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/game/assets")
}
fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}
fn decoded() -> Arc<AnimationAsset> {
	Arc::new(animation_asset::decode(GLB).unwrap())
}
fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 2e-5, "{a} != {b}");
}
fn edit_glb(edit: impl FnOnce(&mut Value, &mut Vec<u8>)) -> Vec<u8> {
	let length = u32::from_le_bytes(GLB[12..16].try_into().unwrap()) as usize;
	let mut doc: Value = serde_json::from_slice(&GLB[20..20 + length]).unwrap();
	let mut bin = GLB[28 + length..].to_vec();
	edit(&mut doc, &mut bin);
	let mut json = serde_json::to_vec(&doc).unwrap();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	while !bin.len().is_multiple_of(4) {
		bin.push(0);
	}
	let total = 12 + 8 + json.len() + 8 + bin.len();
	let mut out = Vec::new();
	for word in [0x46546c67, 2, total as u32, json.len() as u32, 0x4e4f534a] {
		out.extend(word.to_le_bytes());
	}
	out.extend(json);
	out.extend((bin.len() as u32).to_le_bytes());
	out.extend(0x004e4942u32.to_le_bytes());
	out.extend(bin);
	out
}
fn scene(looping: bool) -> SceneData {
	let all = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/animation-room.json"
	))
	.unwrap();
	let mut entity = all
		.entities
		.into_iter()
		.find(|e| e.key.as_str() == "clip-demo")
		.unwrap();
	entity.transform.position = [0.; 3];
	entity.animation.as_mut().unwrap().settings.playback = AnimationPlayback::SingleClip {
		clip_index: 1,
		looping,
	};
	let camera = engine_core::Camera::default();
	SceneData {
		schema_version: engine_core::scene_data::CURRENT_SCHEMA_VERSION,
		bindings: BTreeMap::new(),
		entities: vec![entity],
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
		lights: vec![],
	}
}
fn project(scene: &SceneData) -> ProjectData {
	let mut project = ProjectData::for_scene("Animation", "scene.json", scene).unwrap();
	project.fixed_hz = 20;
	project
}
fn install(session: &mut SimulationSession, asset: Arc<AnimationAsset>) {
	session
		.install_animation_bindings(BTreeMap::from([(key("clip-demo"), asset)]))
		.unwrap();
}

#[test]
fn fixture_decodes_three_clips_and_shared_source_instances_share_one_immutable_asset() {
	let asset = decoded();
	assert_eq!(asset.skin.joints.len(), 2);
	assert_eq!(asset.nodes.len(), 4);
	assert_eq!(asset.primitives.len(), 2);
	assert_eq!(
		asset
			.clips
			.iter()
			.map(|c| c.name.as_str())
			.collect::<Vec<_>>(),
		["Idle", "Walk", "Air"]
	);
	let mut data = scene(true);
	let mut second = data.entities[0].clone();
	second.key = key("copy");
	data.entities.push(second);
	let bindings = animation_asset::prepare_scene(&root(), &data).unwrap();
	assert!(Arc::ptr_eq(
		&bindings[&key("clip-demo")],
		&bindings[&key("copy")]
	));
	let id = AssetId::new("a9a07900-9dc2-4d9f-8ff1-718defec0291").unwrap();
	let metadata = animation_asset::inspect(&root(), &id).unwrap();
	assert_eq!(metadata.source.mesh_node, 3);
	assert_eq!(metadata.clips.len(), 3);
	let mut wrong = metadata.source;
	wrong.expected_sha256 = "0".repeat(64);
	assert!(
		animation_asset::load(&root(), &wrong)
			.unwrap_err()
			.contains("changed")
	);
	data.entities[1]
		.animation
		.as_mut()
		.unwrap()
		.settings
		.source
		.mesh_node = 2;
	assert!(
		animation_asset::prepare_scene(&root(), &data).is_err(),
		"shared cache must still validate each source binding"
	);
}

#[test]
fn decoder_rejects_bad_weights_cubic_sparse_and_nonuniform_basis() {
	let bad_weights = edit_glb(|doc, bin| {
		let accessor = doc["meshes"][0]["primitives"][0]["attributes"]["WEIGHTS_0"]
			.as_u64()
			.unwrap() as usize;
		let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
		let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
		bin[offset..offset + 16].fill(0);
	});
	assert!(
		animation_asset::decode(&bad_weights)
			.unwrap_err()
			.contains("weights")
	);
	let cubic = edit_glb(|doc, _| {
		doc["animations"][0]["samplers"][0]["interpolation"] = json!("CUBICSPLINE")
	});
	assert!(
		animation_asset::decode(&cubic)
			.unwrap_err()
			.contains("CUBICSPLINE")
	);
	let sparse = edit_glb(
		|doc, _| doc["accessors"][0]["sparse"] = json!({"count":1,"indices":{"bufferView":4,"componentType":5123},"values":{"bufferView":0}}),
	);
	assert!(animation_asset::decode(&sparse).is_err());
	let scale = edit_glb(|doc, _| doc["nodes"][0]["scale"] = json!([1, 2, 1]));
	assert!(
		animation_asset::decode(&scale)
			.unwrap_err()
			.contains("Nonuniform")
	);
}

#[test]
fn rest_palette_uses_mesh_local_basis_and_animated_envelope_contains_skinned_vertices() {
	let asset = decoded();
	let rest = asset.nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
	let globals = global_pose(&asset, &rest).unwrap();
	let palette = mesh_local_palette(&asset, &globals).unwrap();
	for matrix in palette {
		for (a, b) in matrix
			.to_cols_array()
			.iter()
			.zip(Mat4::IDENTITY.to_cols_array())
		{
			close(*a, b);
		}
	}
	close(globals[usize::from(asset.skin.mesh_node)].w_axis.x, 0.15);
	for scale in [0.01, 1., 8.] {
		let placement = Mat4::from_scale_rotation_translation(
			Vec3::splat(scale),
			Quat::from_rotation_y(0.4),
			Vec3::new(3., 2., -1.),
		);
		let pose = sample_pose(asset.clone(), 1, 0.5, placement, 7).unwrap();
		assert_eq!(pose.revision, 7);
		let min = pose.bounds.min();
		let max = pose.bounds.max();
		for vertex in asset.primitives.iter().flat_map(|p| &p.vertices) {
			let p = Vec3::from_array(vertex.position);
			let mut skin = Vec3::ZERO;
			for n in 0..4 {
				skin += pose.palette[usize::from(vertex.joints[n])].transform_point3(p)
					* vertex.weights[n];
			}
			let world = (placement * pose.mesh_matrix)
				.transform_point3(skin)
				.to_array();
			for i in 0..3 {
				assert!(
					world[i] >= min[i] && world[i] <= max[i],
					"vertex {world:?} outside {min:?}..{max:?}"
				);
			}
		}
	}
	assert!(checked_similarity_inverse(Mat4::from_scale(Vec3::new(-1., 1., 1.))).is_err());
	let mut shear = Mat4::IDENTITY;
	shear.y_axis.x = 0.3;
	assert!(checked_similarity_inverse(shear).is_err());
}

#[test]
fn sampler_obeys_step_linear_antipodal_quaternions_and_exact_loop_boundary() {
	let mut asset = (*decoded()).clone();
	let start = sample_local(&asset, 1, 0.).unwrap();
	let end = sample_local(&asset, 1, 0.5).unwrap();
	let middle = sample_local(&asset, 1, 0.25).unwrap();
	let node = usize::from(asset.clips[1].tracks[0].node);
	let rotated = Quat::from_array(middle[node].rotation_xyzw) * Vec3::Y;
	close(rotated.x, 0.);
	close(rotated.y, 1.);
	asset.clips[1].tracks[0].interpolation = Interpolation::Step;
	assert_eq!(sample_local(&asset, 1, 0.25).unwrap()[node], start[node]);
	assert_eq!(sample_local(&asset, 1, 0.5).unwrap()[node], end[node]);
	let q = Quat::from_rotation_z(0.7);
	let interpolated = Quat::from_array(short_slerp(q.to_array(), (-q).to_array(), 0.5).unwrap());
	assert!((interpolated * Vec3::X - q * Vec3::X).length() < 1e-5);
	assert_eq!(clip_time(1., 1., true).unwrap(), 0.);
	assert_eq!(clip_time(2., 1., false).unwrap(), 1.);
	assert_eq!(clip_time(8., 0., true).unwrap(), 0.);
	assert!(clip_time(f64::NAN, 1., true).is_err());
}

#[test]
fn animation_alone_schedules_ticks_and_finished_one_shot_reaches_idle() {
	let data = scene(true);
	let mut looping = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	assert!(
		looping.tick(0.05).is_err(),
		"unprepared animation cannot begin"
	);
	install(&mut looping, decoded());
	assert!(looping.needs_tick());
	looping.tick(0.05).unwrap();
	assert_eq!(
		looping
			.animations()
			.pose(&key("clip-demo"))
			.unwrap()
			.elapsed_ticks,
		1
	);
	looping.set_paused(true);
	assert_eq!(looping.tick(0.2).unwrap().steps, 0);
	looping.set_paused(false);
	assert!(looping.needs_tick());
	let data = scene(false);
	let mut once = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	install(&mut once, decoded());
	for _ in 0..20 {
		once.tick(0.05).unwrap();
	}
	assert!(!once.needs_tick());
	assert_eq!(
		once.animations()
			.pose(&key("clip-demo"))
			.unwrap()
			.elapsed_ticks,
		20
	);
	assert_eq!(once.tick(0.05).unwrap().steps, 0);
}

struct Failure(Arc<AtomicUsize>);
impl GameModuleFactory for Failure {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self(self.0.clone())))
	}
}
impl GameModule for Failure {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.0.fetch_add(1, Ordering::Relaxed) == 2 {
			out.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}
#[test]
fn late_substep_failure_preserves_pose_clock_palette_bounds_and_revision() {
	let data = Arc::new(scene(true));
	let mut simulation = SimulationSession::with_factory(
		project(&data),
		data.clone(),
		&Failure(Arc::new(AtomicUsize::new(0))),
	)
	.unwrap();
	install(&mut simulation, decoded());
	let before = simulation
		.animations()
		.pose(&key("clip-demo"))
		.unwrap()
		.clone();
	assert!(simulation.tick(0.15).is_err());
	assert_eq!(simulation.tick_index(), 0);
	let after = simulation.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(after.elapsed_ticks, before.elapsed_ticks);
	assert_eq!(after.revision, before.revision);
	assert_eq!(after.palette, before.palette);
	assert_eq!(after.local, before.local);
	assert_eq!(after.bounds, before.bounds);
	simulation.set_paused(false);
	simulation.tick(0.15).unwrap();
	let mut clean = SimulationSession::new(project(&data), data).unwrap();
	install(&mut clean, decoded());
	clean.tick(0.15).unwrap();
	let a = simulation.animations().pose(&key("clip-demo")).unwrap();
	let b = clean.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(a.elapsed_ticks, b.elapsed_ticks);
	assert_eq!(a.palette, b.palette);
	assert_eq!(a.bounds, b.bounds);
}

#[test]
fn instances_share_assets_but_advance_independent_clip_clocks() {
	let mut data = scene(true);
	let mut second = data.entities[0].clone();
	second.key = key("copy");
	second
		.animation
		.as_mut()
		.unwrap()
		.settings
		.speed_denominator = 2;
	data.entities.push(second);
	let prepared = animation_asset::prepare_scene(&root(), &data).unwrap();
	let mut simulation = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	simulation.install_animation_bindings(prepared).unwrap();
	for _ in 0..20 {
		simulation.tick(0.05).unwrap();
	}
	let normal = simulation.animations().pose(&key("clip-demo")).unwrap();
	let slow = simulation.animations().pose(&key("copy")).unwrap();
	assert!(Arc::ptr_eq(&normal.asset, &slow.asset));
	assert_eq!(normal.elapsed_ticks, 20);
	assert_eq!(slow.elapsed_ticks, 20);
	assert_ne!(normal.local, slow.local);
	assert_eq!(normal.local, sample_local(&normal.asset, 1, 0.).unwrap());
	assert_eq!(slow.local, sample_local(&slow.asset, 1, 0.5).unwrap());
}

#[test]
fn disabled_rest_pose_bounds_follow_accepted_owner_movement() {
	let mut data = scene(true);
	data.entities[0].animation.as_mut().unwrap().enabled = false;
	let mut configured = project(&data);
	configured.add_behavior(engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("clip-demo"),
		speed: 1.,
		space: engine_core::project::BehaviorSpace::World,
	});
	let mut simulation = SimulationSession::new(configured, Arc::new(data)).unwrap();
	install(&mut simulation, decoded());
	let original = simulation
		.animations()
		.pose(&key("clip-demo"))
		.unwrap()
		.clone();
	simulation.set_controlled_input([1., 0., 0.]).unwrap();
	simulation.tick(0.05).unwrap();
	let moved = simulation.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(moved.local, original.local);
	assert_eq!(moved.elapsed_ticks, 0);
	close(moved.bounds.min()[0] - original.bounds.min()[0], 0.05);
	close(moved.bounds.max()[0] - original.bounds.max()[0], 0.05);
}

#[test]
fn embedded_base_color_images_are_decoded_once_and_sampler_subset_is_explicit() {
	let image = image::RgbaImage::from_raw(2, 1, vec![255, 20, 10, 255, 10, 200, 30, 255]).unwrap();
	let mut encoded = std::io::Cursor::new(Vec::new());
	image::DynamicImage::ImageRgba8(image)
		.write_to(&mut encoded, image::ImageFormat::Png)
		.unwrap();
	let encoded = encoded.into_inner();
	let textured = |unsupported_sampler: bool| {
		edit_glb(|doc, bin| {
			let view = doc["bufferViews"].as_array().unwrap().len();
			let offset = bin.len();
			bin.extend(&encoded);
			doc["bufferViews"]
				.as_array_mut()
				.unwrap()
				.push(json!({"buffer":0,"byteOffset":offset,"byteLength":encoded.len()}));
			doc["buffers"][0]["byteLength"] = json!(bin.len());
			doc["images"] = json!([{"bufferView":view,"mimeType":"image/png"}]);
			doc["textures"] = json!([{"source":0,"sampler":0}]);
			doc["samplers"] = json!([{"magFilter":9729,"minFilter":9729,"wrapS":if unsupported_sampler{33071}else{10497},"wrapT":10497}]);
			for material in doc["materials"].as_array_mut().unwrap() {
				material["pbrMetallicRoughness"]["baseColorTexture"] =
					json!({"index":0,"texCoord":0});
			}
		})
	};
	let asset = animation_asset::decode(&textured(false)).unwrap();
	assert_eq!(asset.images.len(), 1);
	assert_eq!((asset.images[0].width, asset.images[0].height), (2, 1));
	assert_eq!(asset.images[0].rgba, [255, 20, 10, 255, 10, 200, 30, 255]);
	assert!(
		asset
			.primitives
			.iter()
			.all(|p| p.base_color_image == Some(0))
	);
	assert_ne!(asset.primitives[0].color, asset.primitives[1].color);
	assert!(
		animation_asset::decode(&textured(true))
			.unwrap_err()
			.contains("linear/repeat")
	);
}
