use runtime::{
	material_graph::{MaterialGraph, MaterialGraphError},
	scene_data::MaterialOverrideData,
};

#[tauri::command]
pub fn editor_compile_material_graph(
	graph: MaterialGraph,
) -> Result<MaterialOverrideData, MaterialGraphError> {
	let base_color_factor = graph.compile_base_color()?;
	Ok(MaterialOverrideData {
		base_color_factor,
		source_graph: Some(Box::new(graph)),
	})
}
