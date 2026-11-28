//! Stable identities from the Blender Character profile.

use std::collections::{BTreeMap, BTreeSet};

use engine_core::animation::AnimationCharacterIdentity;
use serde_json::Value;

use super::rig::DecodedRig;

const JOINT_ID: &str = "game_engine_joint_id";
const CLIP_ID: &str = "game_engine_clip_id";

pub(super) fn decode_character(
	doc: &gltf::Document,
	rig: &DecodedRig<'_>,
) -> Result<Option<AnimationCharacterIdentity>, String> {
	let skin_joints: BTreeSet<_> = rig
		.skin
		.joints
		.iter()
		.map(|&index| rig.nodes[usize::from(index)].source_node)
		.collect();
	let mut joints = BTreeMap::new();
	for node in doc.nodes() {
		let source_node = node.index() as u32;
		if let Some(id) = extra_id(node.extras(), JOINT_ID)? {
			if !skin_joints.contains(&source_node) {
				return Err(format!(
					"Node {source_node} names a Character joint outside the skin"
				));
			}
			joints.insert(source_node, id);
		}
	}
	let clips = doc
		.animations()
		.map(|animation| extra_id(animation.extras(), CLIP_ID))
		.collect::<Result<Vec<_>, _>>()?;
	if joints.is_empty() && clips.iter().all(Option::is_none) {
		return Ok(None);
	}
	let clips = clips
		.into_iter()
		.collect::<Option<Vec<_>>>()
		.ok_or("Character profile requires a stable ID on every animation clip")?;
	if joints.len() != skin_joints.len() {
		return Err("Character profile requires a stable ID on every skin joint".into());
	}
	let root = |mut index: u16| {
		while let Some(parent) = rig.nodes[usize::from(index)].parent {
			index = parent;
		}
		index
	};
	let mesh_root = root(rig.skin.mesh_node);
	let root_node = rig
		.skin
		.joints
		.iter()
		.all(|&index| root(index) == mesh_root)
		.then_some(rig.nodes[usize::from(mesh_root)].source_node);
	let character = AnimationCharacterIdentity {
		clips,
		joints,
		root_node,
	};
	character.validate()?;
	Ok(Some(character))
}

fn extra_id(extras: &gltf::json::Extras, name: &str) -> Result<Option<String>, String> {
	let Some(extras) = extras else {
		return Ok(None);
	};
	let value: Value = serde_json::from_str(extras.get()).map_err(|error| error.to_string())?;
	match value.get(name) {
		None => Ok(None),
		Some(Value::String(id)) if AnimationCharacterIdentity::valid_id(id) => Ok(Some(id.clone())),
		Some(_) => Err(format!("Invalid Character extras.{name}")),
	}
}
