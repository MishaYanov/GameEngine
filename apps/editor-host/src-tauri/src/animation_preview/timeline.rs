use super::{
	authored::{AuthoredClipPreviewDto, ClipAssetIndex, ClipReplacement},
	AnimationPreviewDocument,
};
use crate::editor::{prefab_sources::draft_history::DraftHistory, EditorWorkspace};
use bridge::editor::EditorSnapshotDto;
use runtime::{
	animation::{AuthoredClip, TimelineEdit},
	asset_catalog::AssetId,
	components::ComponentId,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct ClipTimelineDrafts(HashMap<String, ClipDraft>);

impl ClipTimelineDrafts {
	/// A stable snapshot of up to four open drafts, independent of scene revision.
	pub(crate) fn revisions(&self) -> Vec<(String, u64)> {
		let mut revisions: Vec<_> = self
			.0
			.iter()
			.map(|(id, draft)| (id.clone(), draft.history.revision()))
			.collect();
		revisions.sort_unstable();
		revisions
	}
}

#[derive(Clone)]
struct ClipDraft {
	asset: ClipAssetIndex,
	saved: Arc<str>,
	history: DraftHistory,
	generation: u64,
}

#[derive(Clone, Serialize)]
pub struct ClipDraftDto {
	pub draft_id: String,
	pub workspace_generation: u64,
	pub draft_revision: u64,
	pub asset: ClipAssetIndex,
	pub dirty: bool,
	pub can_undo: bool,
	pub can_redo: bool,
	pub clip: AuthoredClip,
}

#[derive(Serialize)]
pub struct ClipDraftSaved {
	pub draft: ClipDraftDto,
	pub snapshot: EditorSnapshotDto,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipDraftOperation {
	Edit { edit: TimelineEdit },
	Undo,
	Redo,
	Discard,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewKeyMove {
	pub key_id: ComponentId,
	pub at: u32,
}

pub(super) struct TimelinePreviewPin {
	pub id: String,
	pub revision: u64,
}

impl ClipDraft {
	fn document(&self) -> Result<AuthoredClip, String> {
		AuthoredClip::from_json(self.history.document().as_bytes())
	}

	fn dirty(&self) -> bool {
		self.history.document().as_ref() != self.saved.as_ref()
	}

	fn dto(&self, id: &str) -> Result<ClipDraftDto, String> {
		Ok(ClipDraftDto {
			draft_id: id.to_owned(),
			workspace_generation: self.generation,
			draft_revision: self.history.revision(),
			asset: self.asset.clone(),
			dirty: self.dirty(),
			can_undo: self.history.can_undo(),
			can_redo: self.history.can_redo(),
			clip: self.document()?,
		})
	}
}

impl EditorWorkspace {
	pub(crate) fn has_dirty_clip_timelines(&self) -> bool {
		self.clip_timeline_drafts.0.values().any(ClipDraft::dirty)
	}

	pub fn open_clip_timeline_draft(
		&mut self,
		asset_id: &AssetId,
		revision: u64,
	) -> Result<ClipDraftDto, String> {
		self.authored_clip_edit_context(revision)?;
		if let Some((id, draft)) = self
			.clip_timeline_drafts
			.0
			.iter()
			.find(|(_, draft)| &draft.asset.asset_id == asset_id)
		{
			return draft.dto(id);
		}
		if self.clip_timeline_drafts.0.len() >= 4 {
			return Err("Close an Animation Clip draft before opening another (limit four)".into());
		}
		let source = self.open_authored_clip_asset(asset_id)?;
		let saved = encode(&source.clip.into_timeline()?)?;
		let draft = ClipDraft {
			asset: source.asset,
			generation: self.generation,
			history: DraftHistory::new(Arc::clone(&saved)),
			saved,
		};
		let id = AssetId::generate().as_str().to_owned();
		let dto = draft.dto(&id)?;
		self.clip_timeline_drafts.0.insert(id, draft);
		Ok(dto)
	}

	pub fn edit_clip_timeline_draft(
		&mut self,
		id: &str,
		expected: u64,
		operation: ClipDraftOperation,
	) -> Result<ClipDraftDto, String> {
		self.authored_clip_edit_context(self.backend.session().revision())?;
		self.timeline_draft(id, expected)?;
		let draft = self
			.clip_timeline_drafts
			.0
			.get_mut(id)
			.ok_or("The Animation Clip draft is closed")?;
		match operation {
			ClipDraftOperation::Edit { edit } => {
				let next = draft.document()?.with_timeline_edit(edit)?;
				draft.history.apply(expected, encode(&next)?)?;
			}
			ClipDraftOperation::Undo => draft.history.undo(expected)?,
			ClipDraftOperation::Redo => draft.history.redo(expected)?,
			ClipDraftOperation::Discard => {
				draft.history.discard(expected, Arc::clone(&draft.saved))?
			}
		}
		draft.dto(id)
	}

	pub fn save_clip_timeline_draft(
		&mut self,
		id: &str,
		expected: u64,
		revision: u64,
	) -> Result<ClipDraftSaved, String> {
		self.authored_clip_edit_context(revision)?;
		let mut next = self.timeline_draft(id, expected)?.clone();
		if next.dirty() {
			let replacement = ClipReplacement {
				asset_id: next.asset.asset_id.clone(),
				fingerprint: next.asset.fingerprint.clone(),
			};
			let saved = self.save_authored_clip_asset(
				&next.asset.path,
				next.document()?,
				Some(replacement),
				revision,
			)?;
			next.asset = saved.document.asset;
			next.saved = Arc::clone(next.history.document());
			let draft = next.dto(id)?;
			self.clip_timeline_drafts.0.insert(id.to_owned(), next);
			Ok(ClipDraftSaved {
				draft,
				snapshot: saved.snapshot,
			})
		} else {
			Ok(ClipDraftSaved {
				draft: next.dto(id)?,
				snapshot: self.backend.snapshot().map_err(|error| error.to_string())?,
			})
		}
	}

	pub fn close_clip_timeline_draft(&mut self, id: &str, discard: bool) -> Result<(), String> {
		if !discard
			&& self
				.clip_timeline_drafts
				.0
				.get(id)
				.is_some_and(ClipDraft::dirty)
		{
			return Err(
				"Save or explicitly discard the Timeline changes before closing the draft".into(),
			);
		}
		self.clip_timeline_drafts.0.remove(id);
		Ok(())
	}

	pub fn open_clip_timeline_preview(
		&mut self,
		id: &str,
		expected: u64,
		scope: Option<AnimationPreviewDocument>,
		revision: u64,
		generation: u64,
		request: u64,
	) -> Result<AuthoredClipPreviewDto, String> {
		self.check_authored_preview_open(revision, generation, request)?;
		let clip = self.timeline_draft(id, expected)?.document()?;
		self.install_authored_clip_preview(
			clip,
			scope,
			revision,
			generation,
			request,
			Some(TimelinePreviewPin {
				id: id.into(),
				revision: expected,
			}),
		)
	}

	pub(super) fn timeline_pin_is_current(&self, owner: &TimelinePreviewPin) -> bool {
		self.timeline_draft(&owner.id, owner.revision).is_ok()
	}

	fn timeline_draft(&self, id: &str, expected: u64) -> Result<&ClipDraft, String> {
		let draft = self
			.clip_timeline_drafts
			.0
			.get(id)
			.filter(|draft| draft.generation == self.generation)
			.ok_or("This Animation Clip draft is closed or belongs to another workspace")?;
		draft
			.history
			.check(expected)
			.map_err(|_| "The Animation Clip draft changed. Review the current keys first.")?;
		Ok(draft)
	}
}

fn encode(clip: &AuthoredClip) -> Result<Arc<str>, String> {
	String::from_utf8(clip.to_json()?)
		.map(Arc::from)
		.map_err(|error| error.to_string())
}
