//! Verified visual snapshots for the single embedded viewport's Prop pane.
use super::EditorWorkspace;
use bridge::viewport::{PropPreviewStateDto, PropPreviewTargetDto};
use runtime::{
	editor::PrefabData,
	scene_catalog::SceneId,
	scene_data::{CameraData, EntityData, CURRENT_SCHEMA_VERSION},
	Camera, EntityKey, SceneData,
};
use std::{
	collections::HashSet,
	sync::{Arc, Weak},
};

enum PreviewOwner {
	Source {
		draft_id: String,
		fingerprint: String,
		draft_revision: u64,
	},
	Instance {
		scene_id: SceneId,
		document: Weak<SceneData>,
	},
}

pub(crate) struct PropPreview {
	owner: PreviewOwner,
	frame: PropPreviewFrame,
}

#[derive(Clone)]
pub(crate) struct PropPreviewFrame {
	pub state: PropPreviewStateDto,
	pub document: Arc<SceneData>,
	pub frame_request: u64,
	pub content_revision: u64,
	pub editable_source: bool,
}

pub(crate) struct PropDraftPreviewRefresh {
	token: String,
	fingerprint: String,
	revision: u64,
	document: Option<Arc<SceneData>>,
}

impl PropPreview {
	fn belongs_to(&self, workspace: &EditorWorkspace) -> bool {
		if self.frame.state.workspace_generation != workspace.generation
			|| workspace.backend.session().is_playing()
		{
			return false;
		}
		match &self.owner {
			PreviewOwner::Source {
				draft_id,
				fingerprint,
				draft_revision,
			} => workspace.prop_preview_source_matches(draft_id, fingerprint, *draft_revision),
			PreviewOwner::Instance { scene_id, document } => workspace
				.backend
				.session()
				.scene_document(scene_id)
				.is_ok_and(|current| Weak::ptr_eq(document, &Arc::downgrade(&current))),
		}
	}
}

impl EditorWorkspace {
	pub(crate) fn open_prop_preview(
		&mut self,
		target: PropPreviewTargetDto,
		revision: u64,
		generation: u64,
		request_id: u64,
	) -> Result<PropPreviewStateDto, String> {
		if generation != self.generation {
			return Err("Prop preview belongs to another workspace".into());
		}
		if request_id <= self.prop_preview_request || request_id > (1_u64 << 53) - 1 {
			return Err("Prop preview request is stale".into());
		}
		self.prop_preview_request = request_id;
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err(
				"Finish the current scene edit and stop Play before opening a Prop preview".into(),
			);
		}
		let (owner, entities, content_revision) = match target {
			PropPreviewTargetDto::Source { draft_id } => {
				let (source, draft_revision) = self.reviewed_prop_preview_source(&draft_id)?;
				(
					PreviewOwner::Source {
						draft_id,
						fingerprint: source.fingerprint,
						draft_revision,
					},
					source.prefab.expanded_entities()?.iter().map(visual_entity).collect(),
					draft_revision,
				)
			}
			PropPreviewTargetDto::Instance { scene_id, root_key } => {
				let scene = session.scene_document(&scene_id)?;
				let entities = instance_entities(&scene, &root_key)?;
				(
					PreviewOwner::Instance {
						scene_id,
						document: Arc::downgrade(&scene),
					},
					entities,
					0,
				)
			}
		};
		let document = preview_document(entities)?;
		let editable_source = matches!(owner, PreviewOwner::Source { .. });
		let state = PropPreviewStateDto {
			token: runtime::asset_catalog::AssetId::generate().as_str().into(),
			workspace_generation: generation,
			revision,
			request_id,
			selection: None,
		};
		self.animation_preview = None;
		self.camera_preview = None;
		self.prop_preview = Some(PropPreview {
			owner,
			frame: PropPreviewFrame {
				state: state.clone(),
				document: Arc::new(document),
				frame_request: 0,
				content_revision,
				editable_source,
			},
		});
		Ok(state)
	}

	pub(crate) fn prepare_prop_draft_preview(
		&self,
		id: &str,
		fingerprint: &str,
		revision: u64,
		prefab: &str,
	) -> Result<Option<PropDraftPreviewRefresh>, String> {
		let Some(preview) = self.prop_preview.as_ref().filter(
			|preview| matches!(&preview.owner, PreviewOwner::Source { draft_id, .. } if draft_id == id),
		) else {
			return Ok(None);
		};
		let document = if preview.frame.content_revision == revision {
			None
		} else {
			let prefab = PrefabData::from_json(prefab)?;
			Some(Arc::new(preview_document(
				prefab.expanded_entities()?.iter().map(visual_entity).collect(),
			)?))
		};
		Ok(Some(PropDraftPreviewRefresh {
			token: preview.frame.state.token.clone(),
			fingerprint: fingerprint.into(),
			revision,
			document,
		}))
	}

	pub(crate) fn apply_prop_draft_preview(&mut self, refresh: Option<PropDraftPreviewRefresh>) {
		let Some(refresh) = refresh else {
			return;
		};
		let Some(preview) = self
			.prop_preview
			.as_mut()
			.filter(|preview| preview.frame.state.token == refresh.token)
		else {
			return;
		};
		let PreviewOwner::Source {
			fingerprint,
			draft_revision,
			..
		} = &mut preview.owner
		else {
			return;
		};
		*fingerprint = refresh.fingerprint;
		*draft_revision = refresh.revision;
		if let Some(document) = refresh.document {
			if preview
				.frame
				.state
				.selection
				.as_ref()
				.is_some_and(|key| !document.entities.iter().any(|entity| &entity.key == key))
			{
				preview.frame.state.selection = None;
			}
			preview.frame.document = document;
			preview.frame.content_revision = refresh.revision;
		}
	}

	pub(crate) fn close_prop_preview(&mut self, token: &str) -> bool {
		if self
			.prop_preview
			.as_ref()
			.is_some_and(|preview| preview.frame.state.token == token)
		{
			self.prop_preview = None;
			true
		} else {
			false
		}
	}

	pub(crate) fn discard_stale_prop_preview(&mut self) -> bool {
		if self
			.prop_preview
			.as_ref()
			.is_some_and(|preview| !preview.belongs_to(self))
		{
			self.prop_preview = None;
			true
		} else {
			false
		}
	}

	pub(crate) fn active_prop_preview(&self) -> Option<PropPreviewFrame> {
		self.prop_preview
			.as_ref()
			.filter(|preview| preview.belongs_to(self))
			.map(|preview| preview.frame.clone())
	}

	pub(crate) fn prop_preview_active(&self) -> bool {
		self.prop_preview
			.as_ref()
			.is_some_and(|preview| preview.belongs_to(self))
	}

	pub(crate) fn prop_preview_is_current(&self, token: &str) -> bool {
		self.prop_preview
			.as_ref()
			.is_some_and(|preview| preview.frame.state.token == token && preview.belongs_to(self))
	}

	pub(crate) fn prop_preview_content_is_current(&self, token: &str, revision: u64) -> bool {
		self.prop_preview.as_ref().is_some_and(|preview| {
			preview.frame.state.token == token
				&& preview.frame.content_revision == revision
				&& preview.belongs_to(self)
		})
	}

	pub(crate) fn prop_preview_edit_draft(
		&self,
		token: &str,
		revision: u64,
	) -> Result<String, String> {
		if !self.prop_preview_content_is_current(token, revision) {
			return Err("The Prop changed during the gesture".into());
		}
		match &self
			.prop_preview
			.as_ref()
			.expect("checked preview owner")
			.owner
		{
			PreviewOwner::Source { draft_id, .. } => Ok(draft_id.clone()),
			PreviewOwner::Instance { .. } => Err("Open the Prop source to edit its objects".into()),
		}
	}

	pub(crate) fn prop_preview_selection_matches(&self, key: &EntityKey) -> bool {
		self.prop_preview
			.as_ref()
			.is_some_and(|preview| preview.frame.state.selection.as_ref() == Some(key))
	}

	pub(crate) fn frame_prop_preview(&mut self, token: &str) -> Result<(), String> {
		let frame = self.prop_preview_frame_mut(token)?;
		frame.frame_request = frame
			.frame_request
			.checked_add(1)
			.ok_or("Prop frame request limit reached")?;
		Ok(())
	}

	pub(crate) fn select_prop_preview(
		&mut self,
		token: &str,
		key: Option<EntityKey>,
	) -> Result<PropPreviewStateDto, String> {
		let frame = self.prop_preview_frame_mut(token)?;
		if key.as_ref().is_some_and(|key| {
			!frame
				.document
				.entities
				.iter()
				.any(|entity| &entity.key == key)
		}) {
			return Err("Object does not belong to this Prop preview".into());
		}
		frame.state.selection = key;
		Ok(frame.state.clone())
	}

	fn prop_preview_frame_mut(&mut self, token: &str) -> Result<&mut PropPreviewFrame, String> {
		if !self.prop_preview_is_current(token) {
			return Err("Prop preview has expired".into());
		}
		Ok(&mut self
			.prop_preview
			.as_mut()
			.expect("validated Prop preview")
			.frame)
	}
}

fn instance_entities(scene: &SceneData, root_key: &EntityKey) -> Result<Vec<EntityData>, String> {
	let root = scene
		.entities
		.iter()
		.find(|entity| &entity.key == root_key)
		.ok_or("Prop instance is missing")?;
	let link = root
		.linked_prefab
		.as_ref()
		.ok_or("Object is not a linked Prop instance")?;
	let members: HashSet<_> = link.entity_map.values().collect();
	let entities = scene
		.entities
		.iter()
		.filter_map(|entity| {
			if &entity.key == root_key {
				let mut anchor = visual_entity(entity).with_parent(None);
				anchor.transform.position = [0.0; 3];
				Some(anchor)
			} else {
				members.contains(&entity.key).then(|| visual_entity(entity))
			}
		})
		.collect();
	Ok(entities)
}

fn visual_entity(entity: &EntityData) -> EntityData {
	// External gameplay references and ownership stay in the source. The
	// preview only needs visual data and collision bounds for child picking.
	let mut visual = EntityData::new(entity.key.clone(), &entity.name, &entity.model_path)
		.with_asset_id(entity.asset_id.clone())
		.with_parent(entity.parent.clone())
		.with_transform(entity.transform.clone())
		.with_renderable(entity.renderable)
		.with_material(entity.material.clone())
		.with_collider(entity.collider.clone())
		.with_mesh_index(entity.mesh_index);
	visual.collision_enabled = entity.collision_enabled;
	visual
}

fn preview_document(entities: Vec<EntityData>) -> Result<SceneData, String> {
	let camera = Camera::default();
	let document = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		bindings: Default::default(),
		entities,
		lights: Vec::new(),
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
	};
	document.validate()?;
	Ok(document)
}
