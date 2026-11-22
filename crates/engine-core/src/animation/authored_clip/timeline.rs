use super::*;

const MAX_MARKERS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarkerId(ComponentId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineMarker {
	pub id: MarkerId,
	pub at: u32,
	pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipKeyValue {
	Position { value: [f32; 3] },
	Rotation { value: [f32; 4] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineEdit {
	MoveKey {
		key_id: ComponentId,
		at: u32,
	},
	InsertKey {
		track: usize,
		at: u32,
		value: Option<ClipKeyValue>,
	},
	RemoveKey {
		key_id: ComponentId,
	},
	SetKeyValue {
		key_id: ComponentId,
		value: ClipKeyValue,
	},
	AddMarker {
		at: u32,
		label: String,
	},
	MoveMarker {
		marker_id: MarkerId,
		at: u32,
	},
	RenameMarker {
		marker_id: MarkerId,
		label: String,
	},
	RemoveMarker {
		marker_id: MarkerId,
	},
}

impl AuthoredClip {
	/// Upgrade a legacy clip only when it first enters the Timeline. The caller
	/// keeps this canonical v2 draft in memory until the author saves it.
	pub fn into_timeline(mut self) -> Result<Self, String> {
		self.validate()?;
		if self.schema_version == 1 {
			self.schema_version = 2;
			for entry in &mut self.tracks {
				match &mut entry.track {
					VisualTrack::LocalPosition { keys, .. } => assign_key_ids(keys),
					VisualTrack::LocalRotation { keys, .. } => assign_key_ids(keys),
				}
			}
		}
		Ok(self)
	}

	pub fn with_timeline_edit(&self, edit: TimelineEdit) -> Result<Self, String> {
		if self.schema_version != 2 {
			return Err("Open the clip in the Timeline before editing its keys".into());
		}
		self.validate()?;
		let mut next = self.clone();
		match edit {
			TimelineEdit::MoveKey { key_id, at } => {
				next.track_with_key_mut(&key_id)?.move_key(&key_id, at)?;
			}
			TimelineEdit::InsertKey { track, at, value } => {
				next.tracks
					.get_mut(track)
					.ok_or("The selected animation track is missing")?
					.track
					.insert_key(at, value)?;
			}
			TimelineEdit::RemoveKey { key_id } => {
				next.track_with_key_mut(&key_id)?.remove_key(&key_id);
			}
			TimelineEdit::SetKeyValue { key_id, value } => {
				next.track_with_key_mut(&key_id)?
					.set_key_value(&key_id, value)?;
			}
			TimelineEdit::AddMarker { at, label } => {
				next.markers.push(TimelineMarker {
					id: MarkerId(ComponentId::generate()),
					at,
					label,
				});
			}
			TimelineEdit::MoveMarker { marker_id, at } => next.marker_mut(&marker_id)?.at = at,
			TimelineEdit::RenameMarker { marker_id, label } => {
				next.marker_mut(&marker_id)?.label = label
			}
			TimelineEdit::RemoveMarker { marker_id } => {
				next.marker_mut(&marker_id)?;
				next.markers.retain(|marker| marker.id != marker_id);
			}
		}
		next.markers
			.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
		next.validate()?;
		Ok(next)
	}

	pub(super) fn validate_timeline_ids(&self) -> Result<(), String> {
		let mut used_keys = BTreeSet::new();
		for scoped in &self.tracks {
			if scoped.track.ids().iter().any(|id| match id {
				Some(id) => self.schema_version != 2 || !used_keys.insert(*id),
				None => self.schema_version == 2,
			}) {
				return Err("Timeline clips require one unique native ID per key".into());
			}
		}
		let mut used_markers = BTreeSet::new();
		if !self.markers.is_empty() && self.schema_version != 2
			|| self.markers.len() > MAX_MARKERS
			|| self.markers.iter().any(|marker| {
				marker.at > self.duration_ticks
					|| marker.label.trim().is_empty()
					|| marker.label.len() > 128
					|| marker.label.chars().any(char::is_control)
					|| !used_markers.insert(&marker.id)
			}) {
			return Err(
				"Timeline markers require unique IDs, in-range times and labels of 1–128 bytes"
					.into(),
			);
		}
		Ok(())
	}

	fn marker_mut(&mut self, id: &MarkerId) -> Result<&mut TimelineMarker, String> {
		self.markers
			.iter_mut()
			.find(|marker| &marker.id == id)
			.ok_or_else(|| "The selected timeline marker is missing".into())
	}

	fn track_with_key_mut(&mut self, id: &ComponentId) -> Result<&mut VisualTrack, String> {
		self.tracks
			.iter_mut()
			.find_map(|scoped| {
				scoped
					.track
					.ids()
					.contains(&Some(id))
					.then_some(&mut scoped.track)
			})
			.ok_or_else(|| "The selected timeline key is missing".into())
	}
}

impl ResolvedAuthoredClip {
	pub fn sample_moved_key(
		&self,
		tick: u32,
		key_id: ComponentId,
		at: u32,
	) -> Result<Vec<VisualPose>, String> {
		let candidate = self
			.clip
			.with_timeline_edit(TimelineEdit::MoveKey { key_id, at })?;
		Self {
			clip: candidate,
			bindings: self.bindings.clone(),
		}
		.sample(tick)
	}
}

impl VisualTrack {
	fn ids(&self) -> Vec<Option<&ComponentId>> {
		match self {
			Self::LocalPosition { keys, .. } => keys.iter().map(|key| key.id.as_ref()).collect(),
			Self::LocalRotation { keys, .. } => keys.iter().map(|key| key.id.as_ref()).collect(),
		}
	}

	fn move_key(&mut self, id: &ComponentId, at: u32) -> Result<(), String> {
		match self {
			Self::LocalPosition { keys, .. } => move_key(keys, id, at),
			Self::LocalRotation { keys, .. } => move_key(keys, id, at),
		}
	}

	fn remove_key(&mut self, id: &ComponentId) {
		match self {
			Self::LocalPosition { keys, .. } => keys.retain(|key| key.id.as_ref() != Some(id)),
			Self::LocalRotation { keys, .. } => keys.retain(|key| key.id.as_ref() != Some(id)),
		}
	}

	fn insert_key(&mut self, at: u32, value: Option<ClipKeyValue>) -> Result<(), String> {
		match (self, value) {
			(Self::LocalPosition { keys, .. }, Some(ClipKeyValue::Position { value })) => {
				insert_or_record(keys, at, value)
			}
			(Self::LocalRotation { keys, .. }, Some(ClipKeyValue::Rotation { value })) => {
				insert_or_record(keys, at, value)
			}
			(
				Self::LocalPosition {
					keys,
					interpolation,
				},
				None,
			) => {
				let (first, last, weight) = key_span(keys, at, *interpolation);
				let value = Vec3::from_array(*first)
					.lerp(Vec3::from_array(*last), weight)
					.to_array();
				insert_key(keys, at, value)?;
			}
			(
				Self::LocalRotation {
					keys,
					interpolation,
				},
				None,
			) => {
				let (first, last, weight) = key_span(keys, at, *interpolation);
				let value = super::super::short_slerp(*first, *last, weight)?;
				insert_key(keys, at, value)?;
			}
			_ => return Err("Recorded values must match the selected track property".into()),
		}
		Ok(())
	}

	fn set_key_value(&mut self, id: &ComponentId, value: ClipKeyValue) -> Result<(), String> {
		match (self, value) {
			(Self::LocalPosition { keys, .. }, ClipKeyValue::Position { value }) => {
				key_mut(keys, id)?.value = value
			}
			(Self::LocalRotation { keys, .. }, ClipKeyValue::Rotation { value }) => {
				key_mut(keys, id)?.value = value
			}
			_ => return Err("Key values must match the selected track property".into()),
		}
		Ok(())
	}
}

fn assign_key_ids<T>(keys: &mut [ClipKey<T>]) {
	for key in keys {
		key.id = Some(ComponentId::generate());
	}
}

fn key_mut<'a, T>(
	keys: &'a mut [ClipKey<T>],
	id: &ComponentId,
) -> Result<&'a mut ClipKey<T>, String> {
	keys.iter_mut()
		.find(|key| key.id.as_ref() == Some(id))
		.ok_or_else(|| "The selected timeline key is missing".into())
}

fn move_key<T>(keys: &mut [ClipKey<T>], id: &ComponentId, at: u32) -> Result<(), String> {
	key_mut(keys, id)?.at = at;
	keys.sort_unstable_by_key(|key| key.at);
	Ok(())
}

fn insert_key<T>(keys: &mut Vec<ClipKey<T>>, at: u32, value: T) -> Result<(), String> {
	let index = keys
		.binary_search_by_key(&at, |key| key.at)
		.err()
		.ok_or("A key already exists at this time")?;
	keys.insert(
		index,
		ClipKey {
			id: Some(ComponentId::generate()),
			at,
			value,
		},
	);
	Ok(())
}

fn insert_or_record<T>(keys: &mut Vec<ClipKey<T>>, at: u32, value: T) {
	match keys.binary_search_by_key(&at, |key| key.at) {
		Ok(index) => keys[index].value = value,
		Err(index) => keys.insert(
			index,
			ClipKey {
				id: Some(ComponentId::generate()),
				at,
				value,
			},
		),
	}
}
