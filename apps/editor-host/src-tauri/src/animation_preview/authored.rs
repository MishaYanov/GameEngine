use super::AnimationPreviewDocument;
use crate::editor::{
	prop_preview::{preview_document, visual_entity, PropPreviewFrame, VisualPreviewFrame},
	EditorWorkspace,
};
use bridge::{editor::EditorSnapshotDto, viewport::PropPreviewStateDto};
use runtime::{
	animation::{AuthoredClip, AuthoredClipOwner, ResolvedAuthoredClip, VisualPose},
	asset_catalog::{AssetId, AssetKind, PersistentAsset},
	assets::project_assets::{self, asset_path, fingerprint, load_catalog},
	components::NO_MESH_PATH,
	editor::PrefabData,
	scene_catalog::SceneId,
	scene_data::EntityData,
	EntityKey, SceneData,
};
use serde::{Deserialize, Serialize};
use std::{
	collections::{HashMap, HashSet},
	fs,
	io::Read,
	sync::Arc,
};

#[derive(Clone, Serialize)]
pub struct ClipAssetIndex {
	pub asset_id: AssetId,
	pub path: String,
	pub fingerprint: String,
	pub revision: u64,
}

#[derive(Serialize)]
pub struct ClipAssetDto {
	pub workspace_generation: u64,
	pub asset: ClipAssetIndex,
	pub clip: AuthoredClip,
}

#[derive(Serialize)]
pub struct ClipAssetSave {
	pub document: ClipAssetDto,
	pub snapshot: EditorSnapshotDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipReplacement {
	pub asset_id: AssetId,
	pub fingerprint: String,
}

#[derive(Serialize)]
pub struct AuthoredClipPreviewDto {
	pub token: String,
	pub workspace_generation: u64,
	pub revision: u64,
	pub request_id: u64,
	pub duration_ticks: u32,
	pub tick: u32,
	pub poses: Vec<VisualPose>,
}

pub(crate) struct AuthoredClipPreview {
	frame: PropPreviewFrame,
	clip: ResolvedAuthoredClip,
	scene_id: SceneId,
	draft: Option<SourceDraftCapture>,
}

struct SourceDraftCapture {
	id: String,
	fingerprint: String,
	revision: u64,
}

impl From<&PersistentAsset> for ClipAssetIndex {
	fn from(asset: &PersistentAsset) -> Self {
		Self {
			asset_id: asset.id.clone(),
			path: asset.path.clone(),
			fingerprint: asset.fingerprint.clone(),
			revision: asset.revision,
		}
	}
}

impl AuthoredClipPreview {
	fn is_current(&self, workspace: &EditorWorkspace) -> bool {
		let session = workspace.backend.session();
		self.frame.state.workspace_generation == workspace.generation
			&& self.frame.state.revision == session.revision()
			&& &self.scene_id == session.active_scene_id()
			&& !session.is_playing()
			&& !session.has_transaction()
			&& self.draft.as_ref().is_none_or(|draft| {
				workspace.prop_preview_source_matches(&draft.id, &draft.fingerprint, draft.revision)
			})
	}

	fn dto(&self, tick: u32, poses: Arc<[VisualPose]>) -> AuthoredClipPreviewDto {
		AuthoredClipPreviewDto {
			token: self.frame.state.token.clone(),
			workspace_generation: self.frame.state.workspace_generation,
			revision: self.frame.state.revision,
			request_id: self.frame.state.request_id,
			duration_ticks: self.clip.duration_ticks(),
			tick,
			poses: poses.as_ref().to_vec(),
		}
	}
}

impl EditorWorkspace {
	pub fn authored_clip_assets(&self) -> Result<Vec<ClipAssetIndex>, String> {
		let catalog = load_catalog(&self.asset_root)?;
		let mut assets: Vec<_> = catalog
			.assets
			.iter()
			.filter(|asset| asset.kind == AssetKind::AnimationClip)
			.map(ClipAssetIndex::from)
			.collect();
		assets.sort_by(|a, b| a.path.cmp(&b.path));
		Ok(assets)
	}

	pub fn open_authored_clip_asset(&self, id: &AssetId) -> Result<ClipAssetDto, String> {
		let catalog = load_catalog(&self.asset_root)?;
		let asset = catalog
			.by_id(id)
			.ok_or("Authored Animation Clip is missing")?;
		let clip = project_assets::read_animation_clip(&self.asset_root, asset)?;
		Ok(ClipAssetDto {
			workspace_generation: self.generation,
			asset: ClipAssetIndex::from(asset),
			clip,
		})
	}

	pub fn save_authored_clip_asset(
		&mut self,
		path: &str,
		clip: AuthoredClip,
		replacement: Option<ClipReplacement>,
		revision: u64,
	) -> Result<ClipAssetSave, String> {
		self.authored_clip_edit_context(revision)?;
		self.backend
			.validate_asset_invalidation()
			.map_err(|error| error.to_string())?;
		match &clip.owner {
			AuthoredClipOwner::Scene { scene_id } => {
				let session = self.backend.session();
				let project = session.project();
				clip.resolve_scene(
					&project,
					scene_id,
					session.scene_document(scene_id)?.as_ref(),
				)?;
				let path = project.scene_path(scene_id)?;
				let bytes =
					self.read_clip_dependency(path, runtime::scene_data::MAX_SCENE_BYTES)?;
				let saved = SceneData::from_json(
					std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
				)?;
				clip.resolve_scene(&project, scene_id, &saved)
					.map_err(|error| {
						format!("Save the target Scene before saving the Animation Clip: {error}")
					})?;
			}
			AuthoredClipOwner::Prop { asset_id } => {
				let source = self.saved_clip_prop(asset_id)?;
				clip.resolve_prop(asset_id, &source).map_err(|error| {
					format!("Save the owning Prop before saving the Animation Clip: {error}")
				})?;
			}
		}
		let change = project_assets::prepare_animation_clip(
			&self.asset_root,
			path,
			&clip,
			replacement
				.as_ref()
				.map(|capture| (&capture.asset_id, capture.fingerprint.as_str())),
		)?;
		let asset = change.publish()?;
		let snapshot = self.asset_published()?;
		Ok(ClipAssetSave {
			document: ClipAssetDto {
				workspace_generation: self.generation,
				asset: ClipAssetIndex::from(&asset),
				clip,
			},
			snapshot,
		})
	}

	pub fn open_authored_clip_preview(
		&mut self,
		id: &AssetId,
		expected_fingerprint: &str,
		scope: Option<AnimationPreviewDocument>,
		revision: u64,
		generation: u64,
		request_id: u64,
	) -> Result<AuthoredClipPreviewDto, String> {
		if generation != self.generation {
			return Err("Authored Animation Clip belongs to another workspace".into());
		}
		if request_id <= self.animation_preview_request || request_id > (1_u64 << 53) - 1 {
			return Err("Authored Animation Clip request is stale".into());
		}
		self.animation_preview_request = request_id;
		self.authored_clip_edit_context(revision)?;
		let asset = self.open_authored_clip_asset(id)?;
		if asset.asset.fingerprint != expected_fingerprint {
			return Err("Authored Animation Clip changed. Reopen it before previewing.".into());
		}
		let clip = asset.clip;
		let scene_id = self.backend.session().active_scene_id().clone();
		let (clip, document, draft) =
			self.resolve_authored_clip_preview(&clip, scope, &scene_id)?;
		let poses: Arc<[VisualPose]> = clip.sample(0)?.into();
		let frame = PropPreviewFrame {
			state: PropPreviewStateDto {
				token: AssetId::generate().as_str().to_owned(),
				workspace_generation: generation,
				revision,
				request_id,
				selection: poses.first().map(|pose| pose.entity_key.clone()),
			},
			document: Arc::new(document),
			frame_request: 0,
			content_revision: 0,
			editable_source: false,
			pose: None,
			visual_pose: Some(VisualPreviewFrame {
				revision: 0,
				poses: Arc::clone(&poses),
			}),
		};
		let preview = AuthoredClipPreview {
			frame,
			clip,
			scene_id,
			draft,
		};
		let result = preview.dto(0, poses);
		self.animation_preview = None;
		self.camera_preview = None;
		self.prop_preview = None;
		self.authored_clip_preview = Some(preview);
		Ok(result)
	}

	pub fn sample_authored_clip_preview(
		&mut self,
		token: &str,
		tick: u32,
	) -> Result<AuthoredClipPreviewDto, String> {
		if !self.authored_clip_aperture_current(token) {
			return Err("Authored Animation Clip preview has expired".into());
		}
		let preview = self
			.authored_clip_preview
			.as_mut()
			.ok_or("Authored Animation Clip preview is closed")?;
		let revision = preview
			.frame
			.visual_pose
			.as_ref()
			.and_then(|pose| pose.revision.checked_add(1))
			.ok_or("Authored Animation Clip sample revision exhausted")?;
		let poses: Arc<[VisualPose]> = preview.clip.sample(tick)?.into();
		preview.frame.visual_pose = Some(VisualPreviewFrame {
			revision,
			poses: Arc::clone(&poses),
		});
		Ok(preview.dto(tick, poses))
	}

	pub(crate) fn active_authored_clip_aperture(&self) -> Option<PropPreviewFrame> {
		self.authored_clip_preview
			.as_ref()
			.filter(|preview| preview.is_current(self))
			.map(|preview| preview.frame.clone())
	}

	pub(crate) fn authored_clip_aperture_current(&self, token: &str) -> bool {
		self.authored_clip_preview
			.as_ref()
			.is_some_and(|preview| preview.frame.state.token == token && preview.is_current(self))
	}

	pub(crate) fn close_authored_clip_preview(&mut self, token: &str) -> bool {
		let matches = self
			.authored_clip_preview
			.as_ref()
			.is_some_and(|preview| preview.frame.state.token == token);
		if matches {
			self.authored_clip_preview = None;
		}
		matches
	}

	pub(crate) fn discard_stale_authored_clip_preview(&mut self) -> bool {
		let stale = self
			.authored_clip_preview
			.as_ref()
			.is_some_and(|preview| !preview.is_current(self));
		if stale {
			self.authored_clip_preview = None;
		}
		stale
	}

	pub(crate) fn frame_authored_clip_preview(&mut self, token: &str) -> Result<(), String> {
		if !self.authored_clip_aperture_current(token) {
			return Err("Authored Animation Clip preview has expired".into());
		}
		let preview = self
			.authored_clip_preview
			.as_mut()
			.ok_or("Authored Animation Clip preview is closed")?;
		preview.frame.frame_request = preview
			.frame
			.frame_request
			.checked_add(1)
			.ok_or("Frame request exhausted")?;
		Ok(())
	}

	fn authored_clip_edit_context(&self, revision: u64) -> Result<(), String> {
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err(
				"Stop Play and finish the current edit before working with an Animation Clip"
					.into(),
			);
		}
		Ok(())
	}

	fn saved_clip_prop(&self, id: &AssetId) -> Result<PrefabData, String> {
		let catalog = load_catalog(&self.asset_root)?;
		let asset = catalog
			.by_id(id)
			.filter(|asset| asset.kind == AssetKind::Prefab)
			.ok_or("The owning Prop asset is missing")?;
		let bytes = self.read_clip_dependency(&asset.path, PrefabData::MAX_JSON_BYTES)?;
		if bytes.len() as u64 != asset.bytes || fingerprint(&bytes) != asset.fingerprint {
			return Err("The owning Prop changed outside the asset catalog".into());
		}
		PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)
	}

	fn read_clip_dependency(&self, path: &str, limit: usize) -> Result<Vec<u8>, String> {
		let path = asset_path(&self.asset_root, path, true)?;
		let mut bytes = Vec::new();
		fs::File::open(path)
			.map_err(|error| error.to_string())?
			.take(limit as u64 + 1)
			.read_to_end(&mut bytes)
			.map_err(|error| error.to_string())?;
		if bytes.len() > limit {
			return Err("Animation Clip dependency exceeds its byte limit".into());
		}
		Ok(bytes)
	}

	fn resolve_authored_clip_preview(
		&self,
		clip: &AuthoredClip,
		scope: Option<AnimationPreviewDocument>,
		active: &SceneId,
	) -> Result<(ResolvedAuthoredClip, SceneData, Option<SourceDraftCapture>), String> {
		use AnimationPreviewDocument::*;
		let session = self.backend.session();
		let project = session.project();
		match scope {
			Some(Source {
				asset_id,
				draft_id,
				draft_revision,
			}) => {
				let (source, actual) = self.reviewed_prop_preview_source(&draft_id)?;
				if source.source_id != asset_id || actual != draft_revision {
					return Err("The Prop draft changed. Reopen the Animation Clip preview.".into());
				}
				let resolved = clip.resolve_prop(&asset_id, &source.prefab)?;
				let frame =
					clip_document(&source.prefab.expanded_entities()?, &resolved.sample(0)?)?;
				Ok((
					resolved,
					frame,
					Some(SourceDraftCapture {
						id: draft_id,
						fingerprint: source.fingerprint,
						revision: actual,
					}),
				))
			}
			Some(Instance { scene_id, root_key }) if &scene_id == active => {
				let scene = session.scene_document(&scene_id)?;
				let resolved =
					clip.resolve_prop_instance(&project, &scene_id, &scene, &root_key)?;
				let document = clip_document(&scene.entities, &resolved.sample(0)?)?;
				Ok((resolved, document, None))
			}
			Some(Scene { scene_id }) if &scene_id == active => {
				let scene = session.scene_document(&scene_id)?;
				let resolved = clip.resolve_scene(&project, &scene_id, &scene)?;
				let document = clip_document(&scene.entities, &resolved.sample(0)?)?;
				Ok((resolved, document, None))
			}
			None => match &clip.owner {
				AuthoredClipOwner::Scene { scene_id } if scene_id == active => {
					let scene = session.scene_document(scene_id)?;
					let resolved = clip.resolve_scene(&project, scene_id, &scene)?;
					let document = clip_document(&scene.entities, &resolved.sample(0)?)?;
					Ok((resolved, document, None))
				}
				AuthoredClipOwner::Prop { asset_id } => {
					let source = self.saved_clip_prop(asset_id)?;
					let resolved = clip.resolve_prop(asset_id, &source)?;
					let frame = clip_document(&source.expanded_entities()?, &resolved.sample(0)?)?;
					Ok((resolved, frame, None))
				}
				_ => Err("Open the Scene that owns this Animation Clip".into()),
			},
			_ => Err("Animation Clip preview belongs to another Scene".into()),
		}
	}
}

fn clip_document(entities: &[EntityData], poses: &[VisualPose]) -> Result<SceneData, String> {
	let by_key: HashMap<_, _> = entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let mut by_parent: HashMap<&EntityKey, Vec<&EntityKey>> = HashMap::new();
	for entity in entities {
		if let Some(parent) = &entity.parent {
			by_parent.entry(parent).or_default().push(&entity.key);
		}
	}
	let mut visible = HashSet::new();
	let mut pending: Vec<_> = poses.iter().map(|pose| &pose.entity_key).collect();
	while let Some(key) = pending.pop() {
		if visible.insert(key) {
			pending.extend(by_parent.get(key).into_iter().flatten());
		}
	}
	let mut ancestors = HashSet::new();
	for key in &visible {
		let mut next = by_key.get(key).and_then(|entity| entity.parent.as_ref());
		while let Some(parent) = next {
			if visible.contains(parent) || !ancestors.insert(parent) {
				break;
			}
			next = by_key.get(parent).and_then(|entity| entity.parent.as_ref());
		}
	}
	preview_document(
		entities
			.iter()
			.filter_map(|entity| {
				if visible.contains(&entity.key) {
					Some(visual_entity(entity).with_collider(None))
				} else if ancestors.contains(&entity.key) {
					Some(
						EntityData::new(entity.key.clone(), &entity.name, NO_MESH_PATH)
							.with_parent(entity.parent.clone())
							.with_transform(entity.transform.clone())
							.with_renderable(false),
					)
				} else {
					None
				}
			})
			.collect(),
	)
}
