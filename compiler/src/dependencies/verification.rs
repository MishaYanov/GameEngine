//! Shared per-owner dependency diagnostics over the retained scene.
use super::{Closure, Path, ProjectData, SceneData, checked_asset_path, fs};
pub use bridge::scene_dependencies::{
	DependencyOpen, DependencyOwner, DependencyProblem, DependencySeverity, DependencyTarget,
};
use engine_core::{
	EntityKey,
	animation::{AnimationClipRef, AnimatorGraph},
	asset_catalog::{AssetId, AssetKind},
	components::{AudioEventBinding, ComponentId, InputActionsRef},
	linked_prefab,
	scene_catalog::SceneId,
	scene_data::EntityData,
};
use std::collections::{HashMap, HashSet};

const MAX_PROBLEMS: usize = 64;

struct Verification<'a> {
	closure: Closure<'a>,
	owners: HashMap<&'a EntityKey, &'a EntityKey>,
	keys: HashSet<&'a EntityKey>,
	problems: Vec<DependencyProblem>,
}

pub fn inspect_scene_dependencies(
	project: &ProjectData,
	scene_id: &SceneId,
	scene: &SceneData,
	asset_root: &Path,
) -> Result<Vec<DependencyProblem>, String> {
	let root = asset_root
		.canonicalize()
		.map_err(|error| error.to_string())?;
	if !root.is_dir()
		|| fs::symlink_metadata(asset_root)
			.map_err(|error| error.to_string())?
			.file_type()
			.is_symlink()
	{
		return Err("Asset root must be a directory without symbolic links".into());
	}
	let catalog = super::catalog::read_catalog(&root)?;
	let mut verification = Verification {
		closure: Closure::new(project, &root, catalog),
		owners: scene
			.entities
			.iter()
			.filter_map(|entity| {
				entity
					.linked_prefab
					.as_ref()
					.map(|link| (&entity.key, link))
			})
			.flat_map(|(root, link)| link.entity_map.values().map(move |key| (key, root)))
			.collect(),
		keys: scene.entities.iter().map(|entity| &entity.key).collect(),
		problems: Vec::new(),
	};
	if let Err(message) = linked_prefab::validate_scene_links(scene) {
		let mut roots = scene
			.entities
			.iter()
			.filter(|entity| entity.linked_prefab.is_some());
		let sole_owner = roots.next().filter(|_| roots.next().is_none());
		verification.push(DependencyProblem {
			code: "prop_materialization_invalid",
			severity: DependencySeverity::Error,
			owner: sole_owner.map(|entity| owner(entity, None)),
			target: None,
			open: sole_owner.map(|entity| DependencyOpen::Prop {
				key: entity.key.clone(),
			}),
			message,
		});
		return Ok(verification.problems);
	}
	verification.closure.capture_clip_owner_pins(scene)?;
	for entity in &scene.entities {
		if verification.problems.len() >= MAX_PROBLEMS {
			verification.push(DependencyProblem {
				code: "dependency_report_limit",
				severity: DependencySeverity::Error,
				owner: None,
				target: None,
				open: None,
				message: "Further dependency diagnostics were omitted.".into(),
			});
			break;
		}
		verification.linked_source(entity);
		verification.entity(scene_id, scene, entity);
	}
	Ok(verification.problems)
}

impl Verification<'_> {
	fn entity(&mut self, scene_id: &SceneId, scene: &SceneData, entity: &EntityData) {
		let initial = self.problems.len();
		if let Some(component) = &entity.sky
			&& let Some(image) = component.settings.background.image()
		{
			self.asset(
				entity,
				&component.id,
				image,
				AssetKind::SkyPanorama,
				"sky_panorama_invalid",
			);
		}
		if let Some(component) = entity.audio_source.as_ref()
			&& let Some(clip) = &component.settings.clip
		{
			self.asset(
				entity,
				&component.id,
				&clip.asset,
				AssetKind::AudioClip,
				"audio_clip_invalid",
			);
		}
		if let Some(component) = &entity.animation {
			let id = &component.settings.source.asset_id;
			let info = self.target(id);
			if let Err(message) = self.closure.animation(entity, component) {
				self.asset_error(
					entity,
					&component.id,
					"animation_source_invalid",
					info,
					component.enabled.then_some(DependencyOpen::Animation {
						key: Some(entity.key.clone()),
					}),
					message,
				);
			}
		}
		if let Some(component) = &entity.animator {
			self.animator(
				scene_id,
				scene,
				entity,
				&component.id,
				&component.settings.asset_id,
				&component.settings,
			);
		}
		if let Err(message) = bridge::animator_graph::validate_actor_audio_event(entity) {
			self.audio_event_error(entity, message.to_owned());
		}
		if let Some(component) = &entity.player_controller {
			if let InputActionsRef::Asset { asset_id } = &component.settings.input {
				self.asset(
					entity,
					&component.id,
					asset_id,
					AssetKind::InputActions,
					"input_actions_invalid",
				);
			}
			if let Some(id) = &component.settings.control_graph {
				let target = self.target(id);
				let result = self
					.closure
					.record_expected(id, AssetKind::ControlsGraph)
					.and_then(|()| {
						let asset = self
							.closure
							.catalog
							.by_id(id)
							.expect("accepted Controls Graph row");
						let graph = bridge::controls_graph::load(self.closure.root, asset)?;
						bridge::controls_graph::compile_for_actor(
							self.closure.root,
							self.closure.project,
							Some(entity),
							&graph,
						)
						.map(|_| ())
					});
				if let Err(message) = result {
					self.asset_error(
						entity,
						&component.id,
						"controls_graph_invalid",
						target,
						Some(DependencyOpen::ControlsGraph {
							asset_id: id.clone(),
						}),
						message,
					);
				}
			}
		}
		if let Some(component) = &entity.camera
			&& let Some(key) = component.settings.rig.target()
			&& !self.keys.contains(key)
		{
			self.asset_error(
				entity,
				&component.id,
				"camera_target_missing",
				DependencyTarget {
					key: Some(key.clone()),
					..Default::default()
				},
				None,
				format!("Camera target {:?} is missing from the scene", key.as_str()),
			);
		}
		if self.problems.len() == initial
			&& let Err(message) = self.closure.entity(entity)
		{
			self.push(DependencyProblem {
				code: "runtime_dependency_invalid",
				severity: DependencySeverity::Error,
				owner: Some(owner(entity, None)),
				target: Some(DependencyTarget {
					asset_id: entity.asset_id.clone(),
					path: relative_path(&entity.model_path),
					key: None,
				}),
				open: Some(self.open(entity, None)),
				message,
			});
		}
	}

	fn animator(
		&mut self,
		scene_id: &SceneId,
		scene: &SceneData,
		entity: &EntityData,
		component: &ComponentId,
		id: &AssetId,
		settings: &engine_core::animation::AnimatorSettings,
	) {
		let animator_target = self.target(id);
		let open = Some(DependencyOpen::Animator {
			asset_id: id.clone(),
		});
		let graph = (|| {
			let asset = self
				.closure
				.catalog
				.by_id(id)
				.ok_or("Animator asset identity is missing")?
				.clone();
			bridge::animator_graph::load(self.closure.root, &asset)
		})();
		let graph = match graph {
			Ok(graph) => graph,
			Err(message) => {
				self.asset_error(
					entity,
					component,
					"animator_asset_invalid",
					animator_target,
					None,
					message,
				);
				return;
			}
		};
		let before = self.problems.len();
		self.animator_clips(entity, component, &graph, &open);
		if before == self.problems.len() {
			let result = self
				.closure
				.record_expected(id, AssetKind::AnimatorGraph)
				.and_then(|()| {
					bridge::animator_graph::prepare_actor_assets(
						self.closure.root,
						self.closure.project,
						scene_id,
						scene,
						entity,
						settings,
					)
				});
			match result {
				Ok(prepared) => {
					if let Err(message) =
						bridge::animator_graph::validate_actor_audio_marker(entity, &prepared)
					{
						self.audio_event_error(entity, message);
					}
				}
				Err(message) => self.asset_error(
					entity,
					component,
					"animator_binding_invalid",
					animator_target,
					open,
					message,
				),
			}
		}
	}

	fn audio_event_error(&mut self, entity: &EntityData, message: String) {
		let Some(source) = &entity.audio_source else {
			return;
		};
		let (code, target, open) = match &source.settings.event {
			Some(AudioEventBinding::AnimationMarker { clip, .. }) => (
				"animation_marker_invalid",
				self.target(clip),
				Some(DependencyOpen::Animation { key: None }),
			),
			_ => (
				"audio_event_binding_invalid",
				DependencyTarget::default(),
				None,
			),
		};
		self.asset_error(entity, &source.id, code, target, open, message);
	}

	fn animator_clips(
		&mut self,
		entity: &EntityData,
		component: &ComponentId,
		graph: &AnimatorGraph,
		open: &Option<DependencyOpen>,
	) {
		let mut seen = HashSet::new();
		for state in &graph.states {
			let (id, fingerprint, kind) = match &state.clip {
				AnimationClipRef::Imported { source, .. } => {
					(&source.asset_id, &source.expected_sha256, AssetKind::Model)
				}
				AnimationClipRef::Authored {
					asset_id,
					expected_sha256,
				} => (asset_id, expected_sha256, AssetKind::AnimationClip),
			};
			if !seen.insert((id, fingerprint)) {
				continue;
			}
			let target = self.target(id);
			let matches_pin = self
				.closure
				.catalog
				.by_id(id)
				.is_some_and(|asset| asset.kind == kind && &asset.fingerprint == fingerprint);
			let result = if matches_pin {
				self.closure.record(id)
			} else {
				Err("Clip identity or captured revision is unavailable".into())
			};
			if let Err(message) = result {
				self.asset_error(
					entity,
					component,
					"animation_clip_invalid",
					target,
					open.clone(),
					format!("Animator state {:?}: {message}", state.id),
				);
			}
		}
	}

	fn linked_source(&mut self, entity: &EntityData) {
		let Some(link) = &entity.linked_prefab else {
			return;
		};
		let pins = std::iter::once((&link.source.source_id, link.source.path.as_str())).chain(
			link.source
				.prefab
				.composition
				.pins
				.iter()
				.map(|pin| (&pin.version.asset_id, pin.path.as_str())),
		);
		let mut seen = HashSet::new();
		for (id, saved_path) in pins {
			if !seen.insert(id) {
				continue;
			}
			let asset = self.closure.catalog.by_id(id);
			let path = asset.map_or(saved_path, |asset| asset.path.as_str());
			let available = asset.is_some_and(|asset| asset.kind == AssetKind::Prefab)
				&& checked_asset_path(self.closure.root, Path::new(path))
					.is_ok_and(|file| file.is_file());
			if !available {
				self.push(DependencyProblem {
                    code: "prop_editing_source_unavailable", severity: DependencySeverity::Warning,
                    owner: Some(owner(entity, None)),
                    target: Some(DependencyTarget { asset_id: Some(id.clone()), path: relative_path(path), key: None }),
                    open: Some(DependencyOpen::Prop {key: entity.key.clone()}),
                    message: "Prop editing source is unavailable. The instance has complete pinned objects.".into(),
                });
			}
		}
	}

	fn asset(
		&mut self,
		entity: &EntityData,
		component: &ComponentId,
		id: &AssetId,
		kind: AssetKind,
		code: &'static str,
	) {
		let target = self.target(id);
		if let Err(message) = self.closure.record_expected(id, kind) {
			self.asset_error(entity, component, code, target, None, message);
		}
	}

	fn target(&self, id: &AssetId) -> DependencyTarget {
		DependencyTarget {
			asset_id: Some(id.clone()),
			path: self
				.closure
				.catalog
				.by_id(id)
				.and_then(|asset| relative_path(&asset.path)),
			key: None,
		}
	}

	fn asset_error(
		&mut self,
		entity: &EntityData,
		component: &ComponentId,
		code: &'static str,
		target: DependencyTarget,
		open: Option<DependencyOpen>,
		message: String,
	) {
		self.push(DependencyProblem {
			code,
			severity: DependencySeverity::Error,
			owner: Some(owner(entity, Some(component))),
			target: Some(target),
			open: Some(self.open(entity, open)),
			message,
		});
	}

	fn open(&self, entity: &EntityData, specific: Option<DependencyOpen>) -> DependencyOpen {
		specific.unwrap_or_else(|| {
			self.owners
				.get(&entity.key)
				.map_or(DependencyOpen::Inspector, |key| DependencyOpen::Prop {
					key: (*key).clone(),
				})
		})
	}

	fn push(&mut self, mut problem: DependencyProblem) {
		if self.problems.len() > MAX_PROBLEMS
			|| self.problems.len() == MAX_PROBLEMS && problem.code != "dependency_report_limit"
		{
			return;
		}
		problem.message = problem.message.chars().take(1200).collect();
		self.problems.push(problem);
	}
}

fn owner(entity: &EntityData, component: Option<&ComponentId>) -> DependencyOwner {
	DependencyOwner {
		key: entity.key.clone(),
		name: entity.name.chars().take(512).collect(),
		component_id: component.cloned(),
	}
}

fn relative_path(path: &str) -> Option<String> {
	engine_core::scene_data::validate_model_path(path).ok()?;
	if path.len() > 1024
		|| path
			.chars()
			.any(|character| character.is_control() || matches!(character, ':' | '\\'))
	{
		return None;
	}
	Some(path.to_owned())
}
