use super::*;
use runtime::animation::{Interpolation, TrackValues};

const KEY_PAGE: usize = 64;

#[derive(Serialize)]
pub struct ImportedTrackSummary {
	index: usize,
	source_node: u32,
	property: &'static str,
	interpolation: &'static str,
	key_count: usize,
}

#[derive(Serialize)]
pub struct ImportedKeyPage {
	token: String,
	clip_index: u32,
	track: usize,
	offset: usize,
	total: usize,
	tracks: Vec<ImportedTrackSummary>,
	keys: Vec<ImportedKey>,
}

#[derive(Serialize)]
struct ImportedKey {
	at_seconds: f32,
	value: Vec<f32>,
}

impl EditorWorkspace {
	pub fn animation_preview_keys(
		&self,
		token: &str,
		track: usize,
		offset: usize,
	) -> Result<ImportedKeyPage, String> {
		let preview = self
			.animation_preview
			.as_ref()
			.filter(|preview| preview.token == token && preview.context(self))
			.ok_or("The imported clip preview has expired")?;
		let asset = &preview.pose.asset;
		let clip = &asset.clips[preview.clip as usize];
		let selected = clip
			.tracks
			.get(track)
			.ok_or("The selected imported animation track is missing")?;
		if offset > selected.times.len() {
			return Err("The imported key page starts beyond the selected track".into());
		}
		let tracks = clip
			.tracks
			.iter()
			.enumerate()
			.map(|(index, track)| ImportedTrackSummary {
				index,
				source_node: asset.nodes[usize::from(track.node)].source_node,
				property: match track.values {
					TrackValues::Translation(_) => "Position",
					TrackValues::Rotation(_) => "Rotation",
					TrackValues::Scale(_) => "Scale",
				},
				interpolation: match track.interpolation {
					Interpolation::Step => "Hold",
					Interpolation::Linear => "Linear",
				},
				key_count: track.times.len(),
			})
			.collect();
		let keys = selected
			.times
			.iter()
			.enumerate()
			.skip(offset)
			.take(KEY_PAGE)
			.map(|(index, time)| ImportedKey {
				at_seconds: *time,
				value: match &selected.values {
					TrackValues::Translation(values) | TrackValues::Scale(values) => {
						values[index].to_vec()
					}
					TrackValues::Rotation(values) => values[index].to_vec(),
				},
			})
			.collect();
		Ok(ImportedKeyPage {
			token: token.into(),
			clip_index: preview.clip,
			track,
			offset,
			total: selected.times.len(),
			tracks,
			keys,
		})
	}
}
