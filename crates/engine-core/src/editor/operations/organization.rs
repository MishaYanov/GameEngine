//! Scene organization edits. Object transforms and selection belong to the scene.

use crate::{
	EntityKey,
	scene_data::{FolderId, FolderRecord, MAX_SCENE_FOLDERS, SceneData, SceneOrganization},
};
use std::collections::HashSet;

pub(super) fn create(
	organization: &mut SceneOrganization,
	name: String,
	parent: Option<FolderId>,
) -> Result<(), String> {
	check_parent(organization, parent.as_ref())?;
	if organization.folders.len() >= MAX_SCENE_FOLDERS {
		return Err("Scene folder limit reached".into());
	}
	let id = loop {
		let id = FolderId::generate();
		if !organization.folders.iter().any(|folder| folder.id == id) {
			break id;
		}
	};
	organization.folders.push(FolderRecord { id, name, parent });
	Ok(())
}

pub(super) fn rename(
	organization: &mut SceneOrganization,
	id: &FolderId,
	name: String,
) -> Result<(), String> {
	let index = folder_index(organization, id)?;
	organization.folders[index].name = name;
	Ok(())
}

pub(super) fn move_folder(
	organization: &mut SceneOrganization,
	id: &FolderId,
	parent: Option<FolderId>,
) -> Result<(), String> {
	let index = folder_index(organization, id)?;
	check_parent(organization, parent.as_ref())?;
	if organization.folders[index].parent == parent {
		return Ok(());
	}
	let mut folder = organization.folders.remove(index);
	folder.parent = parent;
	// The authored vector defines sibling order.
	organization.folders.push(folder);
	Ok(())
}

pub(super) fn move_roots(
	data: &mut SceneData,
	roots: Vec<EntityKey>,
	folder: Option<FolderId>,
) -> Result<(), String> {
	check_parent(&data.organization, folder.as_ref())?;
	let wanted: HashSet<_> = roots.iter().collect();
	let found = data
		.entities
		.iter()
		.filter(|entity| entity.parent.is_none() && wanted.contains(&entity.key))
		.count();
	if found != roots.len() {
		return Err("Only existing scene root objects can be moved into a folder".into());
	}
	for root in roots {
		match &folder {
			Some(folder) => {
				data.organization
					.root_membership
					.insert(root, folder.clone());
			}
			None => {
				data.organization.root_membership.remove(&root);
			}
		}
	}
	Ok(())
}

pub(super) fn remove(organization: &mut SceneOrganization, id: &FolderId) -> Result<(), String> {
	let index = folder_index(organization, id)?;
	let removed = organization.folders.remove(index);
	for folder in &mut organization.folders {
		if folder.parent.as_ref() == Some(id) {
			folder.parent = removed.parent.clone();
		}
	}
	organization.root_membership.retain(|_, folder| {
		if folder != id {
			return true;
		}
		if let Some(parent) = &removed.parent {
			*folder = parent.clone();
			true
		} else {
			false
		}
	});
	Ok(())
}

fn check_parent(organization: &SceneOrganization, parent: Option<&FolderId>) -> Result<(), String> {
	if let Some(parent) = parent {
		folder_index(organization, parent)?;
	}
	Ok(())
}

fn folder_index(organization: &SceneOrganization, id: &FolderId) -> Result<usize, String> {
	organization
		.folders
		.iter()
		.position(|item| &item.id == id)
		.ok_or_else(|| format!("Scene folder {} does not exist", id.as_str()))
}
