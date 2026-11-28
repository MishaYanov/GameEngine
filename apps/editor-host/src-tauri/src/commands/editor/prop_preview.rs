use super::*;
use crate::editor::prop_preview::PropPreviewEnvironment;
use bridge::viewport::{PropPreviewStateDto, PropPreviewTargetDto, ViewportShading};
use runtime::{
	asset_registry::AssetRef,
	assets::{
		gltf_loader::{load_mesh_metadata_in_root, ImportedNormalDetail},
		project_assets::resolve_asset,
	},
	EntityKey,
};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct PropPreviewMaterialsDto {
	model_path: String,
	materials: Vec<PropPreviewMaterialDto>,
}

#[derive(Serialize)]
struct PropPreviewMaterialDto {
	index: Option<usize>,
	name: String,
	normal: Option<PropNormalMapDto>,
	primitives: Vec<PropNormalPrimitiveDto>,
}

#[derive(Serialize)]
struct PropNormalMapDto {
	texture: usize,
	image: usize,
	tex_coord: u32,
	scale: f32,
}

#[derive(Serialize)]
struct PropNormalPrimitiveDto {
	index: usize,
	detail: &'static str,
}

fn inspect_materials(root: &Path, reference: &AssetRef) -> Result<PropPreviewMaterialsDto, String> {
	let reference = resolve_asset(root, reference)?;
	let scene = load_mesh_metadata_in_root(
		&root.join(&reference.path),
		root,
		reference.mesh_index.unwrap_or(0),
	)
		.map_err(|error| format!("Could not inspect {}: {error}", reference.path))?;
	let mut by_material: HashMap<_, Vec<_>> = HashMap::new();
	for primitive in scene.meshes.into_iter().flat_map(|mesh| mesh.primitives) {
		let detail = match primitive.normal_detail {
			ImportedNormalDetail::Absent => "Geometry normals: no normal map is assigned.",
			ImportedNormalDetail::StrengthZero => "Geometry normals: normal map strength is zero.",
			ImportedNormalDetail::ReconstructedFlatFrame => {
				"Reconstructed flat normals and tangent frame."
			}
			ImportedNormalDetail::GeneratedTangents => {
				"Generated tangents from the selected UV map."
			}
			ImportedNormalDetail::AuthoredTangents => "Imported tangents from the model.",
		};
		by_material
			.entry(primitive.material_index)
			.or_default()
			.push(PropNormalPrimitiveDto {
				index: primitive.primitive_index,
				detail,
			});
	}
	let materials = scene
		.materials
		.into_iter()
		.map(|material| {
			let normal = material
				.normal_texture_index
				.zip(material.normal_image_index)
				.zip(material.normal_uv_set.zip(material.normal_scale))
				.map(|((texture, image), (tex_coord, scale))| PropNormalMapDto {
					texture,
					image,
					tex_coord,
					scale,
				});
			PropPreviewMaterialDto {
				index: material.material_index,
				name: material.name,
				normal,
				primitives: by_material
					.remove(&material.material_index)
					.unwrap_or_default(),
			}
		})
		.collect();
	Ok(PropPreviewMaterialsDto {
		model_path: reference.path,
		materials,
	})
}

#[tauri::command]
pub async fn editor_prop_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	target: PropPreviewTargetDto,
	expected_revision: u64,
	expected_generation: u64,
	request_id: u64,
) -> Result<PropPreviewStateDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("prop_preview_open"), move |workspace| {
		workspace.open_prop_preview(target, expected_revision, expected_generation, request_id)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.close_prop_preview(&token);
		Ok(())
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_frame(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.frame_prop_preview(&token)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_environment(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	environment: PropPreviewEnvironment,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.set_prop_preview_environment(&token, environment)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_display(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	shading: ViewportShading,
	show_vertices: bool,
	show_normal_detail: Option<bool>,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.set_prop_preview_display(
			&token,
			shading,
			show_vertices,
			show_normal_detail.unwrap_or(true),
		)
	})
		.await
}

#[tauri::command]
pub async fn editor_prop_preview_materials(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	key: EntityKey,
) -> Result<Option<PropPreviewMaterialsDto>, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let (root, revision, reference) = {
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			let workspace = slot.as_ref().ok_or("Prop preview has expired")?;
			let (revision, reference) = workspace.prop_preview_material_ref(&token, &key)?;
			(workspace.asset_root.clone(), revision, reference)
		};
		let materials = reference
			.map(|reference| inspect_materials(&root, &reference))
			.transpose();
		let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
		if !slot
			.as_ref()
			.is_some_and(|workspace| workspace.prop_preview_content_is_current(&token, revision))
		{
			return Err("Prop changed before its materials could be displayed".into());
		}
		materials
	})
		.await
		.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn editor_prop_preview_select(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	key: Option<EntityKey>,
) -> Result<PropPreviewStateDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.select_prop_preview(&token, key)
	})
	.await
}
