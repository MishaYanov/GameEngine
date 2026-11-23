use super::*;
use crate::editor::prop_preview::{preview_document, visual_entity, PropPreviewFrame};
use bridge::viewport::PropPreviewStateDto;
use runtime::{
	components::{ComponentId, NO_MESH_PATH},
	linked_prefab::PropAddress,
	scene_catalog::SceneId,
	scene_data::EntityData,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationPreviewDocument {
	Scene {
		scene_id: SceneId,
	},
	Source {
		asset_id: AssetId,
		draft_id: String,
		draft_revision: u64,
	},
	Instance {
		scene_id: SceneId,
		root_key: EntityKey,
	},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationWorkspaceTarget {
	pub document: AnimationPreviewDocument,
	pub entity_key: EntityKey,
	pub component_id: ComponentId,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub address: Option<PropAddress>,
}

pub(super) struct ScopedAnimationPreview {
	pub(super) target: AnimationWorkspaceTarget,
	pub(super) frame: PropPreviewFrame,
	fingerprint: Option<String>,
}

impl ScopedAnimationPreview {
	pub(super) fn context(&self, workspace: &EditorWorkspace) -> bool {
		use AnimationPreviewDocument::*;
		match &self.target.document {
			Scene { scene_id } | Instance { scene_id, .. } => {
				workspace.backend.session().active_scene_id() == scene_id
			}
			Source {
				draft_id,
				draft_revision,
				..
			} => self.fingerprint.as_ref().is_some_and(|fingerprint| {
				workspace.prop_preview_source_matches(draft_id, fingerprint, *draft_revision)
			}),
		}
	}
}

type ResolvedAnimationScope = (Vec<EntityData>, Option<PropAddress>, Option<String>);

impl EditorWorkspace {
	pub fn open_animation_workspace_preview(
		&mut self,
		mut target: AnimationWorkspaceTarget,
		clip: u32,
		revision: u64,
		generation: u64,
		request_id: u64,
	) -> Result<AnimationPreviewDto, String> {
		if generation != self.generation {
			return Err("Animation preview belongs to another workspace".into());
		}
		if request_id <= self.animation_preview_request || request_id > (1_u64 << 53) - 1 {
			return Err("Animation preview request is stale".into());
		}
		self.animation_preview_request = request_id;
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err(
				"Stop Play and complete the active edit before previewing animation".into(),
			);
		}
		target.entity_key.validate()?;
		let (entities, address, fingerprint) = self.resolve_animation_scope(&target)?;
		if target.address.is_some() && target.address != address {
			return Err("Animation target address no longer matches the selected object".into());
		}
		target.address = address;
		let document = Arc::new(preview_document(entities)?);
		let selected = document
			.entities
			.last()
			.ok_or("Animation preview object is missing")?;
		let component = selected
			.animation
			.as_ref()
			.ok_or("Apply an Animation component before previewing clips")?;
		let asset = bridge::animation_asset::load(&self.asset_root, &component.settings.source)?;
		asset.validate_settings(&component.settings)?;
		let placement = *document
			.validated_world_matrices()?
			.last()
			.ok_or("Animation placement is missing")?;
		let pose = Arc::new(sample_pose(asset, clip, 0.0, placement, 0)?);
		let token = AssetId::generate().as_str().to_owned();
		let selected_key = selected.key.clone();
		let frame = PropPreviewFrame {
			state: PropPreviewStateDto {
				token: token.clone(),
				workspace_generation: generation,
				revision,
				request_id,
				selection: Some(selected_key.clone()),
			},
			document,
			frame_request: 0,
			content_revision: 0,
			editable_source: false,
			pose: None,
			visual_pose: None,
		};
		let candidate = AnimationPreview {
			token,
			generation,
			revision,
			entity: selected_key,
			clip,
			placement,
			pose,
			isolated: Some(ScopedAnimationPreview {
				target,
				frame,
				fingerprint,
			}),
		};
		let result = candidate.dto(0.0);
		self.prop_preview = None;
		self.authored_clip_preview = None;
		self.camera_preview = None;
		self.animation_preview = Some(candidate);
		Ok(result)
	}

	fn resolve_animation_scope(
		&self,
		target: &AnimationWorkspaceTarget,
	) -> Result<ResolvedAnimationScope, String> {
		use AnimationPreviewDocument::*;
		let session = self.backend.session();
		match &target.document {
			Source {
				asset_id,
				draft_id,
				draft_revision,
			} => {
				let (source, actual_revision) = self.reviewed_prop_preview_source(draft_id)?;
				if asset_id != &source.source_id || *draft_revision != actual_revision {
					return Err(
						"The Prop source or draft revision changed. Reopen the animation preview."
							.into(),
					);
				}
				let projection = source
					.prefab
					.expanded_composition(Some(&source.source_id))?;
				let expanded_key = match target.address.as_ref() {
					Some(
						address @ PropAddress::Object {
							placement_path,
							source_entity,
						},
					) if !placement_path.is_empty() && source_entity == &target.entity_key => {
						address.materialized_key()?
					}
					_ => target.entity_key.clone(),
				};
				let address = projection
					.addresses
					.get(&expanded_key)
					.cloned()
					.ok_or("Animation object is missing from its Prop draft")?;
				Ok((
					isolate_animated_object(
						&projection.entities,
						&expanded_key,
						&target.component_id,
					)?,
					Some(address),
					Some(source.fingerprint),
				))
			}
			Scene { scene_id } | Instance { scene_id, .. } => {
				if session.active_scene_id() != scene_id {
					return Err("Animation preview belongs to another scene".into());
				}
				let scene = session.scene_document(scene_id)?;
				let address = if let Instance { root_key, .. } = &target.document {
					let link = scene
						.entities
						.iter()
						.find(|entity| &entity.key == root_key)
						.and_then(|entity| entity.linked_prefab.as_ref())
						.ok_or("The selected linked Prop instance is missing")?;
					let source_key = link
						.entity_map
						.iter()
						.find_map(|(source, member)| {
							(member == &target.entity_key).then_some(source)
						})
						.ok_or(
							"The animated object does not belong to the selected Prop instance",
						)?;
					let projection = link
						.source
						.prefab
						.expanded_composition(Some(&link.source.source_id))?;
					Some(
						projection
							.addresses
							.get(source_key)
							.cloned()
							.ok_or("The pinned animation address is missing")?,
					)
				} else {
					if scene
						.entities
						.iter()
						.filter_map(|entity| entity.linked_prefab.as_ref())
						.any(|link| {
							link.entity_map
								.values()
								.any(|key| key == &target.entity_key)
						}) {
						return Err(
							"Select the owning Prop instance to preview this animated object"
								.into(),
						);
					}
					None
				};
				Ok((
					isolate_animated_object(
						&scene.entities,
						&target.entity_key,
						&target.component_id,
					)?,
					address,
					None,
				))
			}
		}
	}

	pub(crate) fn active_animation_aperture(&self) -> Option<PropPreviewFrame> {
		let preview = self
			.animation_preview
			.as_ref()
			.filter(|preview| preview.context(self))?;
		let mut frame = preview.isolated.as_ref()?.frame.clone();
		frame.pose = Some((preview.entity.clone(), Arc::clone(&preview.pose)));
		Some(frame)
	}

	pub(crate) fn has_active_animation_aperture(&self) -> bool {
		self.animation_preview
			.as_ref()
			.is_some_and(|preview| preview.isolated.is_some() && preview.context(self))
	}

	pub(crate) fn animation_aperture_is_current(&self, token: &str) -> bool {
		self.animation_preview.as_ref().is_some_and(|preview| {
			preview.token == token && preview.isolated.is_some() && preview.context(self)
		})
	}

	pub(crate) fn frame_animation_aperture(&mut self, token: &str) -> Result<(), String> {
		if !self.animation_aperture_is_current(token) {
			return Err("Animation preview has expired".into());
		}
		let scoped = self
			.animation_preview
			.as_mut()
			.and_then(|preview| preview.isolated.as_mut())
			.ok_or("Animation preview is closed")?;
		scoped.frame.frame_request = scoped
			.frame
			.frame_request
			.checked_add(1)
			.ok_or("Animation frame request limit reached")?;
		Ok(())
	}
}

fn isolate_animated_object(
	entities: &[EntityData],
	key: &EntityKey,
	component_id: &ComponentId,
) -> Result<Vec<EntityData>, String> {
	let by_key: HashMap<_, _> = entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let object = by_key.get(key).ok_or("The animated object is missing")?;
	let animation = object
		.animation
		.as_ref()
		.filter(|component| &component.id == component_id)
		.ok_or("The selected Animation component is missing or changed")?;
	if !object.renderable || !object.has_mesh_component() {
		return Err("Animation preview requires a visible mesh on the selected object".into());
	}
	let mut visual = visual_entity(object).with_collider(None);
	let mut animation = animation.clone();
	animation.enabled = true;
	visual.animation = Some(animation);
	let mut parent = object.parent.as_ref();
	let mut isolated = vec![visual];
	while let Some(key) = parent {
		if isolated.len() >= entities.len() {
			return Err("The animated object has an invalid hierarchy".into());
		}
		let entity = by_key
			.get(key)
			.ok_or("An animation preview parent is missing")?;
		isolated.push(
			EntityData::new(entity.key.clone(), &entity.name, NO_MESH_PATH)
				.with_parent(entity.parent.clone())
				.with_transform(entity.transform.clone())
				.with_renderable(false),
		);
		parent = entity.parent.as_ref();
	}
	isolated.reverse();
	Ok(isolated)
}
