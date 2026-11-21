use super::{EditorSession, SceneDraft};
use crate::{
	scene_catalog::{MAX_PROJECT_SCENES, SceneId},
	scene_data::SceneData,
};
use std::{
	collections::HashMap,
	sync::{Arc, Weak},
};

pub const MAX_RETAINED_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default)]
pub(in crate::editor) struct DocumentCostCache {
	pub(super) entries: HashMap<usize, CachedDocumentCost>,
}

pub(super) struct CachedDocumentCost {
	// A weak reference protects pointer identity without retaining the document.
	document: Weak<SceneData>,
	bytes: usize,
	used: bool,
}

impl DocumentCostCache {
	pub(super) fn cost(&mut self, data: &Arc<SceneData>) -> Result<usize, String> {
		let identity = Arc::as_ptr(data) as usize;
		if let Some(entry) = self.entries.get_mut(&identity) {
			entry.used = true;
			return Ok(entry.bytes);
		}
		let bytes = document_cost(data)?;
		self.entries.insert(
			identity,
			CachedDocumentCost {
				document: Arc::downgrade(data),
				bytes,
				used: true,
			},
		);
		Ok(bytes)
	}
}

pub(super) fn document_cost(data: &SceneData) -> Result<usize, String> {
	Ok(data
		.serialized_size()?
		.saturating_mul(4)
		.saturating_add(std::mem::size_of::<SceneData>()))
}

impl EditorSession {
	pub(in crate::editor) fn check_document_budget(
		&mut self,
		replacement: Option<(&SceneId, &SceneData)>,
	) -> Result<(), String> {
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&self.drafts,
			replacement,
			&mut self.document_costs,
		)
		.map(|_| ())
	}

	pub(in crate::editor) fn document_budget(
		active: &SceneId,
		data: &Arc<SceneData>,
		saved: &Arc<SceneData>,
		drafts: &HashMap<SceneId, SceneDraft>,
		replacement: Option<(&SceneId, &SceneData)>,
		cache: &mut DocumentCostCache,
	) -> Result<usize, String> {
		if drafts.len() + 1 > MAX_PROJECT_SCENES {
			return Err("Retained scene count limit exceeded".into());
		}
		for entry in cache.entries.values_mut() {
			entry.used = false;
		}
		let result = (|| {
			// Conservative logical accounting, with the same current/saved sharing
			// rules as before. Mutable replacement candidates are never cached.
			let mut bytes = if let Some((_, next)) = replacement.filter(|(id, _)| *id == active) {
				document_cost(next)?
			} else {
				cache.cost(data)?
			};
			if !Arc::ptr_eq(data, saved) || replacement.is_some_and(|(id, _)| id == active) {
				bytes = bytes.saturating_add(cache.cost(saved)?);
			}
			for (id, draft) in drafts {
				let next = replacement
					.filter(|(key, _)| *key == id)
					.map(|(_, data)| data);
				bytes = bytes.saturating_add(match next {
					Some(next) => document_cost(next)?,
					None => cache.cost(&draft.data)?,
				});
				if !Arc::ptr_eq(&draft.data, &draft.saved) || next.is_some() {
					bytes = bytes.saturating_add(cache.cost(&draft.saved)?);
				}
			}
			if bytes > MAX_RETAINED_DOCUMENT_BYTES {
				return Err("Retained scene document byte budget exceeded".into());
			}
			Ok(bytes)
		})();
		// Also prune after errors: rejected candidates and externally held retired
		// snapshots must not make this cache grow beyond retained document count.
		cache
			.entries
			.retain(|_, entry| entry.used && entry.document.strong_count() != 0);
		result
	}
}
