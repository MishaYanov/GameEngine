//! Validate a revision of retained authored scenes through the native Play path.
use super::{animation_resolver, EditorWorkspace};
use runtime::{
	game_session::GameSession, project::ProjectData, scene_catalog::SceneId,
	simulation_session::GameModuleFactory, EntityKey, SceneData,
};
use serde::Serialize;
use std::{
	collections::hash_map::RandomState,
	hash::{BuildHasher, Hasher},
	io::{self, Write},
	path::PathBuf,
	sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneScope {
	Intro,
	Arena,
	Scene,
	Project,
}

#[derive(Serialize)]
pub struct BindingSuggestion {
	pub entity_key: EntityKey,
	pub entity_name: String,
}

#[derive(Serialize)]
pub struct SceneIssue {
	pub scope: SceneScope,
	pub scene_id: Option<SceneId>,
	pub message: String,
	pub role: Option<String>,
	pub suggestion: Option<BindingSuggestion>,
}

#[derive(Serialize)]
pub struct CheckedScene {
	pub id: SceneId,
	pub content_stamp: String,
	pub checked: bool,
}

#[derive(Serialize)]
pub struct SceneVerificationReport {
	pub generation: u64,
	pub revision: u64,
	pub active_scene_id: SceneId,
	pub scenes: Vec<CheckedScene>,
	pub issues: Vec<SceneIssue>,
}

pub struct SceneVerificationInput {
	generation: u64,
	revision: u64,
	active_scene_id: SceneId,
	project: Arc<ProjectData>,
	scenes: Vec<(SceneId, Arc<SceneData>)>,
	asset_root: PathBuf,
	check_gate: Arc<Mutex<()>>,
}

impl EditorWorkspace {
	pub fn capture_scene_verification(
		&self,
		expected_generation: Option<u64>,
		expected_revision: u64,
		expected_scene_id: &SceneId,
	) -> Result<SceneVerificationInput, String> {
		self.check_scene_verification_current(
			expected_generation,
			expected_revision,
			expected_scene_id,
		)?;
		let session = self.backend.session();
		let project = session.project_document();
		let mut scenes = project
			.scenes
			.iter()
			.map(|entry| {
				let id = entry.id().clone();
				Ok((id.clone(), session.scene_document(&id)?))
			})
			.collect::<Result<Vec<_>, String>>()?;
		scenes.sort_by(|(a, _), (b, _)| {
			(a != &project.startup_scene_id, a.as_str())
				.cmp(&(b != &project.startup_scene_id, b.as_str()))
		});

		Ok(SceneVerificationInput {
			generation: self.generation,
			revision: expected_revision,
			active_scene_id: expected_scene_id.clone(),
			project,
			scenes,
			asset_root: self.asset_root.clone(),
			check_gate: Arc::clone(&self.scene_verification_gate),
		})
	}

	pub fn check_scene_verification_current(
		&self,
		expected_generation: Option<u64>,
		expected_revision: u64,
		expected_scene_id: &SceneId,
	) -> Result<(), String> {
		self.check_revision(expected_revision)?;
		let session = self.backend.session();
		if expected_generation.is_some_and(|generation| generation != self.generation)
			|| expected_scene_id != session.active_scene_id()
		{
			return Err(
				"The checked workspace or active scene changed. Check Scenes again.".into(),
			);
		}
		if session.is_playing() || session.has_transaction() {
			return Err("Stop Play and finish the current edit before checking scenes.".into());
		}
		Ok(())
	}
}

impl SceneVerificationInput {
	/// Construct disposable sessions off the workspace lock. A single check per
	/// workspace keeps repeated reads of animation assets bounded.
	pub fn verify(&self) -> Result<SceneVerificationReport, String> {
		let _check = self
			.check_gate
			.try_lock()
			.map_err(|_| "A scene check is already running.")?;
		let mut report = SceneVerificationReport {
			generation: self.generation,
			revision: self.revision,
			active_scene_id: self.active_scene_id.clone(),
			scenes: Vec::with_capacity(self.scenes.len()),
			issues: vec![],
		};
		let factory = self
			.project
			.validate()
			.and_then(|()| gameplay_catalog::resolve(&self.project));
		let route = if factory.is_ok() {
			gameplay_catalog::cinder_oath::intro_route(&self.project)?
		} else {
			None
		};
		let descriptor = route
			.as_ref()
			.map(|_| gameplay_catalog::cinder_oath::descriptor());
		let project_stamp = content_stamp(&*self.project)?;
		let resolver = animation_resolver(&self.asset_root);

		if let Err(message) = &factory {
			report.issues.push(SceneIssue {
				scope: SceneScope::Project,
				scene_id: None,
				message: message.clone(),
				role: None,
				suggestion: None,
			});
		}

		for (id, document) in &self.scenes {
			let stamp = format!("{project_stamp}-{}", content_stamp(&**document)?);
			report.scenes.push(CheckedScene {
				id: id.clone(),
				content_stamp: stamp,
				checked: factory.is_ok(),
			});
			let Ok(factory) = &factory else { continue };
			let scope = match &route {
				Some((intro, _)) if intro == id => SceneScope::Intro,
				Some((_, arena)) if arena == id => SceneScope::Arena,
				_ => SceneScope::Scene,
			};

			let roles = descriptor.as_ref().and_then(|module| {
				let name = match scope {
					SceneScope::Intro => "intro",
					SceneScope::Arena => "arena",
					_ => return None,
				};
				module.scene_roles.get(name)
			});

			let mut missing_roles = false;
			for role in roles
				.into_iter()
				.flatten()
				.filter(|role| !document.bindings.contains_key(&role.name))
			{
				missing_roles = true;
				report.issues.push(SceneIssue {
					scope,
					scene_id: Some(id.clone()),
					message: format!("Assign the {} role. {}", role.label, role.requirement_hint),
					role: Some(role.name.clone()),
					suggestion: self.suggest_binding(id, document, scope, &role.name, factory),
				});
			}
			if missing_roles {
				continue;
			}

			let checked =
				self.open_scene(id, Arc::clone(document), factory)
					.and_then(|mut game| {
						game.set_animation_resolver(Arc::clone(&resolver))?;
						game.active().require_animation_ready()
					});
			if let Err(message) = checked {
				report.issues.push(SceneIssue {
					scope,
					scene_id: Some(id.clone()),
					message,
					role: None,
					suggestion: None,
				});
			}
		}
		Ok(report)
	}

	fn open_scene(
		&self,
		id: &SceneId,
		document: Arc<SceneData>,
		factory: &Arc<dyn GameModuleFactory>,
	) -> Result<GameSession, String> {
		GameSession::with_factory_for_scene(
			Arc::clone(&self.project),
			id.clone(),
			document,
			Arc::clone(factory),
		)
	}

	fn suggest_binding(
		&self,
		id: &SceneId,
		document: &SceneData,
		scope: SceneScope,
		role: &str,
		factory: &Arc<dyn GameModuleFactory>,
	) -> Option<BindingSuggestion> {
		let key = match (scope, role) {
			(SceneScope::Intro, "camera_track") => &document.camera.follow.as_ref()?.target,
			(SceneScope::Arena, "player") => &document.camera.orbit.as_ref()?.target,
			_ => return None,
		};
		let entity = document.entities.iter().find(|entity| &entity.key == key)?;
		let mut candidate = document.clone();
		candidate.bindings.insert(role.into(), key.clone());
		self.open_scene(id, Arc::new(candidate), factory).ok()?;
		Some(BindingSuggestion {
			entity_key: key.clone(),
			entity_name: entity.name.clone(),
		})
	}
}

/// An ephemeral comparison token for a visited scene. Hash typed authored data
/// without retaining a second copy or serializing the token as a file identity.
fn content_stamp(value: &impl Serialize) -> Result<String, String> {
	static HASHER: OnceLock<RandomState> = OnceLock::new();
	let mut writer = HashWriter(HASHER.get_or_init(RandomState::new).build_hasher());
	let mut serializer = serde_json::Serializer::new(&mut writer);
	value
		.serialize(&mut serializer)
		.map_err(|error| error.to_string())?;
	Ok(format!("{:016x}", writer.0.finish()))
}

struct HashWriter<H>(H);

impl<H: Hasher> Write for HashWriter<H> {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.0.write(bytes);
		Ok(bytes.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}
