//! Capture authored context for the Agent without consulting disk, Play, or previews.
use super::EditorWorkspace;
use runtime::{
	components::{ComponentId, ComponentRecord, MAX_COMPONENTS_PER_OBJECT},
	editor::PrefabData,
	linked_prefab::PropAddress,
	scene_catalog::SceneId,
	scene_data::{EntityData, TransformData},
	EntityKey, SceneData,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const MAX_KEY_BYTES: usize = 256;
const MAX_NAME_CHARACTERS: usize = 128;
const MAX_COMPONENT_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
	tag = "kind",
	rename_all = "snake_case",
	rename_all_fields = "camelCase",
	deny_unknown_fields
)]
pub enum InspectorAddress {
	Scene {
		scene: SceneId,
		object: EntityKey,
	},
	Prop {
		draft_id: String,
		source_object: EntityKey,
	},
	Linked {
		scene: SceneId,
		root: EntityKey,
		address: PropAddress,
	},
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContextRequest {
	pub workspace_generation: u64,
	pub revision: u64,
	pub content_revision: u64,
	pub address: InspectorAddress,
	pub component_id: Option<ComponentId>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextErrorCode {
	Stale,
	Missing,
	Unsupported,
	Unavailable,
	Forbidden,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AgentContextError {
	pub code: AgentContextErrorCode,
	pub message: &'static str,
}

impl AgentContextError {
	pub(crate) fn new(code: AgentContextErrorCode, message: &'static str) -> Self {
		Self { code, message }
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum InspectionCapability {
	InspectObject,
	InspectComponent,
}

#[derive(Debug, Serialize)]
pub struct AgentComponentSummary {
	pub id: ComponentId,
	pub type_id: &'static str,
	pub schema_version: u32,
	pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct AgentObjectSummary {
	pub workspace_generation: u64,
	pub revision: u64,
	pub content_revision: u64,
	pub address: InspectorAddress,
	pub entity_key: EntityKey,
	pub name: String,
	pub transform: TransformData,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub parent: Option<EntityKey>,
	pub components: Vec<AgentComponentSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub selected_component: Option<ComponentRecord>,
	capabilities: [InspectionCapability; 2],
}

enum AuthoredDocument {
	Scene(Arc<SceneData>),
	Prop(Arc<str>),
}

pub(crate) struct CapturedAgentContext {
	request: AgentContextRequest,
	document: AuthoredDocument,
}

impl EditorWorkspace {
	/// Keep the shared workspace locked only while validating scope and cloning a document handle.
	pub(crate) fn capture_agent_context(
		&self,
		request: AgentContextRequest,
	) -> Result<CapturedAgentContext, AgentContextError> {
		use AgentContextErrorCode::{Missing, Stale, Unsupported};

		let session = self.backend.session();
		if request.workspace_generation != self.generation || request.revision != session.revision()
		{
			return Err(AgentContextError::new(
				Stale,
				"The Inspector workspace or revision changed",
			));
		}
		if session.is_playing() || session.has_transaction() {
			return Err(AgentContextError::new(
				Unsupported,
				"Agent context requires an authored selection outside Play and grouped edits",
			));
		}
		let document = match &request.address {
			InspectorAddress::Scene { scene, .. } | InspectorAddress::Linked { scene, .. } => {
				if session.active_scene_id() != scene
					|| session.scene_revision() != request.content_revision
				{
					return Err(AgentContextError::new(
						Stale,
						"The authored Scene or content revision changed",
					));
				}
				AuthoredDocument::Scene(session.scene_document(scene).map_err(|_| {
					AgentContextError::new(Missing, "The authored Scene is unavailable")
				})?)
			}
			InspectorAddress::Prop { draft_id, .. } => {
				if draft_id.len() > MAX_KEY_BYTES {
					return Err(AgentContextError::new(
						Unsupported,
						"The Prop draft identifier is oversized",
					));
				}
				let (document, revision) =
					self.agent_prop_draft_document(draft_id).ok_or_else(|| {
						AgentContextError::new(
							Missing,
							"The selected Prop draft is closed or unavailable",
						)
					})?;
				if revision != request.content_revision {
					return Err(AgentContextError::new(
						Stale,
						"The Prop draft content revision changed",
					));
				}
				AuthoredDocument::Prop(document)
			}
		};
		Ok(CapturedAgentContext { request, document })
	}
}

impl CapturedAgentContext {
	pub(crate) fn resolve(self) -> Result<AgentObjectSummary, AgentContextError> {
		use AgentContextErrorCode::{Missing, Unsupported};

		let Self { request, document } = self;
		match (&request.address, document) {
			(InspectorAddress::Scene { object, .. }, AuthoredDocument::Scene(scene)) => {
				check_key(object)?;
				let entity = find_entity(&scene.entities, object)?;
				if scene
					.entities
					.iter()
					.filter_map(|item| item.linked_prefab.as_ref())
					.any(|link| link.entity_map.values().any(|member| member == object))
				{
					return Err(AgentContextError::new(
						Unsupported,
						"Use the linked Prop address for this object",
					));
				}
				summarize(request, entity)
			}
			(InspectorAddress::Linked { root, address, .. }, AuthoredDocument::Scene(scene)) => {
				let entity = linked_entity(&scene, root, address)?;
				summarize(request, entity)
			}
			(InspectorAddress::Prop { source_object, .. }, AuthoredDocument::Prop(json)) => {
				check_key(source_object)?;
				let draft = PrefabData::from_json(&json).map_err(|_| {
					AgentContextError::new(Unsupported, "The captured Prop draft could not be read")
				})?;
				let entity = find_entity(&draft.entities, source_object)?;
				summarize(request, entity)
			}
			_ => Err(AgentContextError::new(
				Missing,
				"The captured authoring document is unavailable",
			)),
		}
	}
}

fn check_key(key: &EntityKey) -> Result<(), AgentContextError> {
	if key.as_str().len() > MAX_KEY_BYTES || key.validate().is_err() {
		return Err(AgentContextError::new(
			AgentContextErrorCode::Unsupported,
			"The Inspector object key is invalid or oversized",
		));
	}
	Ok(())
}

fn find_entity<'a>(
	entities: &'a [EntityData],
	key: &EntityKey,
) -> Result<&'a EntityData, AgentContextError> {
	entities
		.iter()
		.find(|entity| &entity.key == key)
		.ok_or_else(|| {
			AgentContextError::new(
				AgentContextErrorCode::Missing,
				"The addressed object is unavailable",
			)
		})
}

fn linked_entity<'a>(
	scene: &'a SceneData,
	root: &EntityKey,
	address: &PropAddress,
) -> Result<&'a EntityData, AgentContextError> {
	use AgentContextErrorCode::{Missing, Unsupported};

	check_key(root)?;
	let link = find_entity(&scene.entities, root)?
		.linked_prefab
		.as_ref()
		.ok_or_else(|| {
			AgentContextError::new(Unsupported, "The addressed root is not a linked Prop")
		})?;
	let source_key = match address {
		PropAddress::Object {
			placement_path,
			source_entity,
		} if placement_path.is_empty() => {
			check_key(source_entity)?;
			// A materialized nested key is not a direct source member.
			find_entity(&link.source.prefab.entities, source_entity)?;
			source_entity.clone()
		}
		_ => address.materialized_key().map_err(|_| {
			AgentContextError::new(Unsupported, "The Prop address exceeds its supported scope")
		})?,
	};
	let key = link.entity_map.get(&source_key).ok_or_else(|| {
		AgentContextError::new(
			Missing,
			"The addressed member is absent from the pinned Prop",
		)
	})?;
	find_entity(&scene.entities, key)
}

fn summarize(
	request: AgentContextRequest,
	entity: &EntityData,
) -> Result<AgentObjectSummary, AgentContextError> {
	use AgentContextErrorCode::{Missing, Unsupported};

	check_key(&entity.key)?;
	if let Some(parent) = &entity.parent {
		check_key(parent)?;
	}
	let records = entity.component_records();
	if records.len() > MAX_COMPONENTS_PER_OBJECT {
		return Err(AgentContextError::new(
			Unsupported,
			"The addressed object exceeds the component budget",
		));
	}
	let components = records
		.iter()
		.map(|record| AgentComponentSummary {
			id: record.id.clone(),
			type_id: record.type_id(),
			schema_version: record.schema_version,
			enabled: record.enabled,
		})
		.collect();
	let selected_component = request
		.component_id
		.as_ref()
		.map(|id| {
			let record = records
				.into_iter()
				.find(|record| &record.id == id)
				.ok_or_else(|| {
					AgentContextError::new(
						Missing,
						"The addressed object does not own that component",
					)
				})?;
			if serde_json::to_vec(&record)
				.map_or(true, |encoded| encoded.len() > MAX_COMPONENT_BYTES)
			{
				return Err(AgentContextError::new(
					Unsupported,
					"The component exceeds the Agent context byte budget",
				));
			}
			Ok(record)
		})
		.transpose()?;
	Ok(AgentObjectSummary {
		workspace_generation: request.workspace_generation,
		revision: request.revision,
		content_revision: request.content_revision,
		address: request.address,
		entity_key: entity.key.clone(),
		name: entity.name.chars().take(MAX_NAME_CHARACTERS).collect(),
		transform: entity.transform.clone(),
		parent: entity.parent.clone(),
		components,
		selected_component,
		capabilities: [
			InspectionCapability::InspectObject,
			InspectionCapability::InspectComponent,
		],
	})
}
