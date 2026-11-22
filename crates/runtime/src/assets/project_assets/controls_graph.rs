use super::{AssetId, AssetKind, PreparedAssetChange, input_actions::prepare_typed_input_asset};
use engine_core::input::{CONTROL_GRAPH_SUFFIX, ControlGraph, MAX_CONTROL_GRAPH_BYTES};
use std::path::Path;

pub fn prepare_control_graph(
	root: &Path,
	destination: &str,
	previous: Option<(&AssetId, &str)>,
	graph: &ControlGraph,
) -> Result<PreparedAssetChange, String> {
	if !destination
		.to_ascii_lowercase()
		.ends_with(CONTROL_GRAPH_SUFFIX)
	{
		return Err("Controls Graph assets use .controls.json".into());
	}
	prepare_typed_input_asset(
		root,
		destination,
		previous,
		graph.to_json()?,
		AssetKind::ControlsGraph,
		MAX_CONTROL_GRAPH_BYTES,
	)
}
