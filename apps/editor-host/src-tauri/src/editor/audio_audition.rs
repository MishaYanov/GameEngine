use super::EditorWorkspace;
use runtime::{
	asset_catalog::{AssetId, PersistentAsset},
	assets::project_assets,
	components::{AudioSourceComponent, ComponentId},
	scene_catalog::SceneId,
	EntityKey,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioAuditionTarget {
	Scene {
		scene_id: SceneId,
		key: EntityKey,
		component_id: ComponentId,
	},
	Prop {
		preview_token: String,
		key: EntityKey,
		component_id: ComponentId,
	},
}

#[derive(Default)]
pub(crate) struct AudioAuditionLane {
	request_id: u64,
	request: Option<AuditionRequest>,
}

struct AuditionRequest {
	token: AssetId,
	target: AudioAuditionTarget,
	workspace_generation: u64,
	revision: u64,
	stamp: AuditionStamp,
}

#[derive(Debug, PartialEq)]
struct AuditionStamp {
	clip: AssetId,
	gain: f32,
	prop_revision: Option<u64>,
}

pub(crate) struct PreparedAudition {
	pub token: AssetId,
	pub root: PathBuf,
	pub clip: PersistentAsset,
}

#[derive(Serialize)]
pub struct AudioAuditionDto {
	token: AssetId,
	bytes: Vec<u8>,
	gain: f32,
}

impl EditorWorkspace {
	pub(crate) fn prepare_audio_audition(
		&mut self,
		target: AudioAuditionTarget,
		generation: u64,
		revision: u64,
		request_id: u64,
	) -> Result<PreparedAudition, String> {
		if generation != self.generation {
			return Err("Audio Source audition belongs to another workspace".into());
		}
		if request_id <= self.audio_audition.request_id || request_id > (1_u64 << 53) - 1 {
			return Err("Audio Source audition request is stale".into());
		}
		self.audio_audition.request_id = request_id;
		self.audio_audition.request = None;
		self.check_revision(revision)?;
		let stamp = self.audition_stamp(&target)?;
		let catalog = project_assets::load_catalog(&self.asset_root)?;
		let clip = catalog.audio_clip(&stamp.clip)?.clone();
		let token = AssetId::generate();
		self.audio_audition.request = Some(AuditionRequest {
			token: token.clone(),
			target,
			workspace_generation: generation,
			revision,
			stamp,
		});
		Ok(PreparedAudition {
			token,
			root: self.asset_root.clone(),
			clip,
		})
	}

	pub(crate) fn finish_audio_audition(
		&mut self,
		token: &AssetId,
		bytes: Vec<u8>,
	) -> Result<AudioAuditionDto, String> {
		self.discard_stale_audio_audition();
		let request = self
			.audio_audition
			.request
			.as_ref()
			.filter(|request| &request.token == token)
			.ok_or("Audio Source selection changed during audition")?;
		Ok(AudioAuditionDto {
			token: token.clone(),
			bytes,
			gain: request.stamp.gain,
		})
	}

	pub(crate) fn stop_audio_audition(&mut self, token: &AssetId) {
		if self
			.audio_audition
			.request
			.as_ref()
			.is_some_and(|request| &request.token == token)
		{
			self.audio_audition.request = None;
		}
	}

	pub(crate) fn discard_stale_audio_audition(&mut self) {
		let stale = self.audio_audition.request.as_ref().is_some_and(|request| {
			request.workspace_generation != self.generation
				|| self.backend.session().revision() != request.revision
				|| self.audition_stamp(&request.target).as_ref() != Ok(&request.stamp)
		});
		if stale {
			self.audio_audition.request = None;
		}
	}

	fn audition_stamp(&self, target: &AudioAuditionTarget) -> Result<AuditionStamp, String> {
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err("Finish the scene edit and stop Play before auditioning audio".into());
		}
		let (document, key, component_id, prop_revision) = match target {
			AudioAuditionTarget::Scene {
				scene_id,
				key,
				component_id,
			} => {
				if session.active_scene_id() != scene_id
					|| !session.selection().contains(key)
					|| self.prop_preview_active()
				{
					return Err("Select the Audio Source in the active Scene".into());
				}
				(session.authored_document(), key, component_id, None)
			}
			AudioAuditionTarget::Prop {
				preview_token,
				key,
				component_id,
			} => {
				let frame = self
					.active_prop_preview()
					.filter(|frame| {
						frame.state.token == *preview_token
							&& frame.state.selection.as_ref() == Some(key)
							&& self.prop_preview_is_current(preview_token)
					})
					.ok_or("Select the Audio Source in the open Prop workspace")?;
				(
					frame.document,
					key,
					component_id,
					Some(frame.content_revision),
				)
			}
		};
		let source = document
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.and_then(|entity| entity.audio_source.as_deref())
			.filter(|source| &source.id == component_id)
			.ok_or("The selected object has no matching Audio Source")?;
		audition_settings(source, prop_revision)
	}
}

fn audition_settings(
	source: &AudioSourceComponent,
	prop_revision: Option<u64>,
) -> Result<AuditionStamp, String> {
	if !source.enabled {
		return Err("Enable the selected Audio Source before previewing it".into());
	}
	let clip = source
		.settings
		.clip
		.as_ref()
		.ok_or("Assign an Audio Clip before previewing it")?;
	Ok(AuditionStamp {
		clip: clip.asset.clone(),
		gain: source.settings.gain,
		prop_revision,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::project_location::ProjectLocation;
	use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest};
	use runtime::{audio_clip::AudioClipRef, components::ComponentData};
	use std::fs;

	struct Fixture(PathBuf);
	impl Drop for Fixture {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}

	fn run(workspace: &mut EditorWorkspace, command: Command) {
		workspace
			.execute_receipt(EditorCommandRequest {
				command,
				expected_revision: workspace.backend.session().revision(),
			})
			.unwrap();
	}

	#[test]
	fn scene_audition_checks_asset_identity_and_discards_late_reads_and_old_stops() {
		let fixture = Fixture(std::env::temp_dir().join(format!(
			"audio-source-audition-{}",
			AssetId::generate().as_str()
		)));
		fs::create_dir(&fixture.0).unwrap();
		let project =
			compiler::create_authoring_project("Audition test", fixture.0.to_str().unwrap())
				.unwrap();
		let wave = fixture.0.join("bell.wav");
		fs::write(
			&wave,
			include_bytes!("../../../../../projects/cinder-oath/assets/audio/blade-hit.wav"),
		)
		.unwrap();
		let location = ProjectLocation::from_manifest(&project.join("project.json")).unwrap();
		let prepared =
			project_assets::prepare_audio_clip_import(location.assets(), &wave, "bell.wav")
				.unwrap();
		let asset = prepared.asset().id.clone();
		prepared.publish().unwrap();
		let mut workspace = EditorWorkspace::open_project_with_notifier(location, || {}).unwrap();
		run(&mut workspace, Command::CreateAudioSource);
		let session = workspace.backend.session();
		let key = session.selection()[0].clone();
		let scene_id = session.active_scene_id().clone();
		let mut component = session
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == key)
			.unwrap()
			.component_records()
			.remove(0);
		let target = AudioAuditionTarget::Scene {
			scene_id,
			key: key.clone(),
			component_id: component.id.clone(),
		};
		let generation = workspace.generation;
		let revision = session.revision();
		assert!(workspace
			.prepare_audio_audition(target.clone(), generation, revision, 1)
			.err()
			.unwrap()
			.contains("Assign"));
		let ComponentData::AudioSource { settings } = &mut component.data else {
			panic!("created non-audio component")
		};
		settings.clip = Some(AudioClipRef {
			asset: AssetId::generate(),
		});
		let result = workspace.execute_receipt(EditorCommandRequest {
			expected_revision: revision,
			command: Command::ReplaceComponent {
				key: key.as_str().into(),
				component: component.clone(),
			},
		});
		assert!(result.is_err());
		assert_eq!(workspace.backend.session().revision(), revision);
		let ComponentData::AudioSource { settings } = &mut component.data else {
			unreachable!()
		};
		settings.clip = Some(AudioClipRef { asset });
		settings.gain = 0.4;
		run(
			&mut workspace,
			Command::ReplaceComponent {
				key: key.as_str().into(),
				component,
			},
		);
		let revision = workspace.backend.session().revision();
		let first = workspace
			.prepare_audio_audition(target.clone(), generation, revision, 2)
			.unwrap();
		let second = workspace
			.prepare_audio_audition(target, generation, revision, 3)
			.unwrap();
		let bytes = project_assets::load_audio_clip(&second.root, &second.clip)
			.unwrap()
			.into_audition_wav();
		workspace.stop_audio_audition(&first.token);
		assert!(workspace
			.finish_audio_audition(&first.token, bytes.clone())
			.is_err());
		assert_eq!(
			workspace
				.finish_audio_audition(&second.token, bytes.clone())
				.unwrap()
				.gain,
			0.4
		);
		assert_eq!(workspace.backend.session().revision(), revision);
		run(&mut workspace, Command::Select { keys: vec![] });
		assert!(workspace
			.finish_audio_audition(&second.token, bytes)
			.is_err());
	}
}
