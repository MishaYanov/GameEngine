//! Diagnostic owners and repair routes shared by Check Scenes and build preflight.
use engine_core::{
	EntityKey,
	asset_catalog::AssetId,
	components::{ComponentId, ScriptComponent, ScriptIssue},
	scene_data::EntityData,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySeverity {
	Error,
	Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyOwner {
	pub key: EntityKey,
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub component_id: Option<ComponentId>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DependencyTarget {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub asset_id: Option<AssetId>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub key: Option<EntityKey>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DependencyOpen {
	Inspector,
	Script {
		#[serde(skip_serializing_if = "Option::is_none")]
		field: Option<String>,
	},
	Prop {
		key: EntityKey,
	},
	Animation {
		#[serde(skip_serializing_if = "Option::is_none")]
		key: Option<EntityKey>,
	},
	Animator {
		asset_id: AssetId,
	},
	ControlsGraph {
		asset_id: AssetId,
	},
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyProblem {
	pub code: &'static str,
	pub severity: DependencySeverity,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub owner: Option<DependencyOwner>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub target: Option<DependencyTarget>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub open: Option<DependencyOpen>,
	#[serde(skip)]
	pub message: String,
}

impl DependencyProblem {
	pub fn for_script(entity: &EntityData, script: &ScriptComponent, issue: ScriptIssue) -> Self {
		let (field, target) = match &issue {
			ScriptIssue::InvalidReference { field, target, .. } => (
				Some(field.clone()),
				target.clone().map(|key| DependencyTarget {
					key: Some(key),
					..Default::default()
				}),
			),
			_ => (None, None),
		};
		Self {
			code: issue.code(),
			severity: DependencySeverity::Error,
			owner: Some(DependencyOwner {
				key: entity.key.clone(),
				name: entity.name.chars().take(512).collect(),
				component_id: Some(script.id.clone()),
			}),
			target,
			open: Some(DependencyOpen::Script { field }),
			message: issue.at_attachment(&entity.key, &script.id),
		}
	}
}
