use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind, PersistentAsset},
	assets::project_assets,
	input::{
		CompiledControlGraph, ControlGraph, ControlGraphFrame, ControlGraphOutput, InputActionKind,
		ResolvedInputActions, MAX_GAME_INPUT_ACTIONS,
	},
	EntityKey,
};
use std::collections::BTreeSet;

#[derive(Serialize)]
pub struct ControlsGraphAssetDto {
	asset: PersistentAsset,
	graph: ControlGraph,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlsGraphCaptureDto {
	id: AssetId,
	fingerprint: String,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlProbeAction {
	Button {
		action: String,
		held: bool,
		pressed: bool,
		released: bool,
	},
	Vector2 {
		action: String,
		value: [f32; 2],
	},
	Delta2 {
		action: String,
		value: [f32; 2],
		#[serde(default)]
		rate: [f32; 2],
	},
}

#[derive(Serialize)]
pub struct ControlsGraphProbeDto {
	order: Vec<String>,
	active_node_ids: Vec<String>,
	output: ControlGraphOutput,
}

#[derive(Default)]
struct ProbeFrame {
	actions: ResolvedInputActions,
	pressed: BTreeSet<String>,
	released: BTreeSet<String>,
}

fn check(workspace: &EditorWorkspace, generation: u64, revision: u64) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Controls Graph operation belongs to a closed project".into());
	}
	workspace.check_revision(revision)
}

#[tauri::command]
pub async fn editor_list_control_graphs(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<Vec<PersistentAsset>, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		Ok(catalog
			.assets
			.into_iter()
			.filter(|asset| asset.kind == AssetKind::ControlsGraph)
			.collect())
	})
	.await
}

#[tauri::command]
pub async fn editor_open_control_graph(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<ControlsGraphAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		let asset = catalog
			.by_id(&asset_id)
			.ok_or("Controls Graph asset identity is unavailable")?;
		let graph = bridge::controls_graph::load(&current.asset_root, asset)?;
		Ok(ControlsGraphAssetDto {
			asset: asset.clone(),
			graph,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_save_control_graph(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	destination: String,
	capture: Option<ControlsGraphCaptureDto>,
	graph: ControlGraph,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<ControlsGraphAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_control_graph"), move |current| {
		check(current, expected_generation, expected_revision)?;
		if current.backend.session().is_playing() || current.backend.session().has_transaction() {
			return Err(
				"Finish the current edit and stop Play before saving the Controls Graph".into(),
			);
		}
		current.validate_asset_destination(&destination)?;
		let prepared = project_assets::prepare_control_graph(
			&current.asset_root,
			&destination,
			capture
				.as_ref()
				.map(|item| (&item.id, item.fingerprint.as_str())),
			&graph,
		)?;
		let asset = prepared.publish()?;
		Ok(ControlsGraphAssetDto { asset, graph })
	})
	.await
}

#[tauri::command]
#[expect(
	clippy::too_many_arguments,
	reason = "Keep the existing Tauri IPC fields at the top level of the request"
)]
pub async fn editor_probe_controls_graph(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: Option<AssetId>,
	fingerprint: Option<String>,
	entity_key: Option<EntityKey>,
	graph: ControlGraph,
	actions: Vec<ControlProbeAction>,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<ControlsGraphProbeDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let session = current.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err(
				"Stop Play and finish the current edit before testing a Controls Graph".into(),
			);
		}
		match (asset_id, fingerprint) {
			(Some(id), Some(fingerprint)) => {
				let catalog = project_assets::load_catalog(&current.asset_root)?;
				let asset = catalog
					.by_id(&id)
					.filter(|asset| {
						asset.kind == AssetKind::ControlsGraph && asset.fingerprint == fingerprint
					})
					.ok_or("Controls Graph changed after opening; reopen it before testing")?;
				bridge::controls_graph::load(&current.asset_root, asset)?;
			}
			(None, None) => {}
			_ => {
				return Err(
					"Controls Graph test requires both asset identity and fingerprint".into(),
				)
			}
		}
		let scene = session.authored_document();
		let actor = entity_key
			.as_ref()
			.map(|key| {
				scene
					.entities
					.iter()
					.find(|actor| &actor.key == key && actor.player_controller.is_some())
					.ok_or("Selected Player Controller no longer exists")
			})
			.transpose()?;
		let compiled = bridge::controls_graph::compile_for_actor(
			&current.asset_root,
			session.project(),
			actor,
			&graph,
		)?;
		let probe = ProbeFrame::from_actions(&compiled, actions)?;
		let (output, active) = compiled.evaluate_with_activity(&ControlGraphFrame {
			actions: &probe.actions,
			pressed: &probe.pressed,
			released: &probe.released,
		});
		Ok(ControlsGraphProbeDto {
			order: compiled.order().map(str::to_owned).collect(),
			active_node_ids: compiled
				.active_node_ids(active)
				.map(str::to_owned)
				.collect(),
			output,
		})
	})
	.await
}

impl ControlProbeAction {
	fn input(&self) -> (&str, InputActionKind) {
		match self {
			Self::Button { action, .. } => (action, InputActionKind::Button),
			Self::Vector2 { action, .. } => (action, InputActionKind::Vector2),
			Self::Delta2 { action, .. } => (action, InputActionKind::Delta2),
		}
	}
}

impl ProbeFrame {
	fn from_actions(
		compiled: &CompiledControlGraph,
		samples: Vec<ControlProbeAction>,
	) -> Result<Self, String> {
		if samples.len() > MAX_GAME_INPUT_ACTIONS {
			return Err("Controls Graph test accepts at most 24 action samples".into());
		}
		let mut seen = BTreeSet::new();
		for sample in &samples {
			let (action, kind) = sample.input();
			if !seen.insert(action) || compiled.input_kind(action) != Some(kind) {
				return Err(format!(
					"Duplicate, unknown, or wrongly typed action sample {action:?}"
				));
			}
		}
		let mut frame = Self::default();
		for sample in samples {
			match sample {
				ControlProbeAction::Button {
					action,
					held,
					pressed,
					released,
				} => {
					if held {
						frame.actions.buttons.insert(action.clone());
					}
					if pressed {
						frame.pressed.insert(action.clone());
					}
					if released {
						frame.released.insert(action);
					}
				}
				ControlProbeAction::Vector2 { action, value } => {
					require_finite(&action, value)?;
					frame.actions.vectors.insert(action, value);
				}
				ControlProbeAction::Delta2 {
					action,
					value,
					rate,
				} => {
					require_finite(&action, value)?;
					require_finite(&action, rate)?;
					frame.actions.deltas.insert(action.clone(), value);
					frame.actions.look_rates.insert(action, rate);
				}
			}
		}
		Ok(frame)
	}
}

fn require_finite(action: &str, value: [f32; 2]) -> Result<(), String> {
	if value.into_iter().all(f32::is_finite) {
		Ok(())
	} else {
		Err(format!("Action sample {action:?} requires finite axes"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeMap;

	#[test]
	fn probe_rejects_duplicate_or_mismatched_samples_and_keeps_short_taps() {
		let graph = ControlGraph::from_json(
			br#"{
			"schema_version": 1,
			"nodes": [
				{"id":"source","data":{"kind":"action","action":"jump","value":"button"}},
				{"id":"sink","data":{"kind":"jump","input":"source"}}
			]
		}"#,
		)
		.unwrap()
		.compile(
			&BTreeMap::from([("jump".into(), InputActionKind::Button)]),
			None,
		)
		.unwrap();
		let tap = || ControlProbeAction::Button {
			action: "jump".into(),
			held: false,
			pressed: true,
			released: true,
		};
		assert!(ProbeFrame::from_actions(&graph, vec![tap(), tap()]).is_err());
		assert!(ProbeFrame::from_actions(
			&graph,
			vec![ControlProbeAction::Vector2 {
				action: "jump".into(),
				value: [0.0; 2],
			}]
		)
		.is_err());
		assert!(require_finite("look", [f32::INFINITY, 0.0]).is_err());
		let frame = ProbeFrame::from_actions(&graph, vec![tap()]).unwrap();
		let (output, activity) = graph.evaluate_with_activity(&ControlGraphFrame {
			actions: &frame.actions,
			pressed: &frame.pressed,
			released: &frame.released,
		});
		let jump = output.intent.jump;
		assert!(!jump.held && jump.pressed && jump.released);
		assert!(graph.active_node_ids(activity).eq(["source", "sink"]));
	}
}
