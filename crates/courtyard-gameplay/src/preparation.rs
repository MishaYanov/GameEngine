use std::sync::Arc;

use super::{
	CampaignProgress, FINISH_SCENE_ID,
	chapters::ChapterRun,
	runtime::CampaignModule,
	settings::{InteractionSettings, RouteGateSettings, SceneInteractions, validate_trigger},
};
use engine_core::{
	EntityKey,
	game_progress::ProgressState,
	project::{BehaviorSettings, ProjectData},
	scene_catalog::SceneId,
	simulation_session::{
		ConfiguredBehaviorFactory, GameModule, GameModuleFactory, PreparedModuleConfig,
	},
};

pub struct CampaignFactory;
impl GameModuleFactory for CampaignFactory {
	fn prefab_references(
		&self,
		scene_settings: &serde_json::Value,
	) -> Result<engine_core::simulation_session::ModuleSettingReferences, String> {
		let settings: SceneInteractions = serde_json::from_value(scene_settings.clone())
			.map_err(|error| format!("Courtyard prefab settings: {error}"))?;
		let mut references = engine_core::simulation_session::ModuleSettingReferences::default();
		match settings {
			SceneInteractions::Course {
				key_trigger,
				exit_trigger,
				destination_scene,
				exit_blocker,
			} => {
				references
					.entities
					.insert("/key_trigger".into(), key_trigger);
				references
					.entities
					.insert("/exit_trigger".into(), exit_trigger);
				if let Some(blocker) = exit_blocker {
					references.entities.insert("/exit_blocker".into(), blocker);
				}
				references
					.scenes
					.insert("/destination_scene".into(), destination_scene);
			}
			SceneInteractions::Finish { finish_trigger } => {
				references
					.entities
					.insert("/finish_trigger".into(), finish_trigger);
			}
		}
		Ok(references)
	}
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(CampaignProgress::default())
	}

	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Err("Courtyard Run requires prepared scene data".into())
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		let settings = InteractionSettings::from_project(config.project())?;
		let campaign = settings
			.as_ref()
			.and_then(|settings| settings.campaign.as_ref());
		let chapter_index = campaign.and_then(|campaign| campaign.index(config.scene_id()));
		let interactions = settings
			.as_ref()
			.and_then(|settings| settings.scenes.get(config.scene_id().as_str()))
			.cloned();
		let route_gate = settings
			.as_ref()
			.and_then(|settings| settings.route_gate.as_ref())
			.filter(|gate| gate.scene_id == *config.scene_id())
			.cloned();
		let is_finish = matches!(&interactions, Some(SceneInteractions::Finish { .. }))
			|| (campaign.is_none()
			&& interactions.is_none()
			&& config.scene_id().as_str() == FINISH_SCENE_ID);
		let (roles, finish) = if is_finish {
			// Finish scene: validate its player and finish marker.
			(None, Some(FinishRoles::from_config(&config)?))
		} else if campaign.is_some() && chapter_index.is_none() {
			(None, None)
		} else {
			// Other scenes: keep the existing course rules.
			let destination = match &interactions {
				Some(SceneInteractions::Course {
					destination_scene, ..
				}) => Some(destination_scene),
				_ => None,
			};
			(CourseRoles::from_config(&config, destination)?, None)
		};
		match &interactions {
			Some(SceneInteractions::Course {
				key_trigger,
				exit_trigger,
				exit_blocker,
				..
			}) => {
				let roles = roles
					.as_ref()
					.ok_or("Course interactions require Player, Key, and Exit bindings")?;
				validate_trigger(&config, key_trigger, &roles.player, "Key trigger")?;
				validate_trigger(&config, exit_trigger, &roles.player, "Exit trigger")?;
				if let Some(blocker) = exit_blocker {
					if blocker == &roles.player
						|| blocker == key_trigger
						|| blocker == exit_trigger
						|| blocker == &roles.key
					{
						return Err("Exit blocker must be separate from Player, Key, and interaction sensors".into());
					}
					let entity = config
						.document()
						.entities
						.iter()
						.find(|entity| &entity.key == blocker)
						.ok_or("Exit blocker references a missing entity")?;
					if entity
						.collider
						.as_ref()
						.is_none_or(|collider| collider.interaction().sensor)
					{
						return Err("Exit blocker needs a blocking collider".into());
					}
				}
				if let Some(gate) = &route_gate {
					validate_route_gate(
						&config,
						gate,
						roles,
						key_trigger,
						exit_trigger,
						exit_blocker.as_ref(),
					)?;
				}
			}
			Some(SceneInteractions::Finish { finish_trigger }) => validate_trigger(
				&config,
				finish_trigger,
				&finish.as_ref().ok_or("Finish roles missing")?.player,
				"Finish trigger",
			)?,
			None => {}
		}

		let base = ConfiguredBehaviorFactory.create_prepared(config)?;

		engine_logger::debug!(target: "courtyard_gameplay", "Campaign scene prepared: finish={is_finish}, has_course_roles={}, has_interactions={}", roles.is_some(), interactions.is_some());
		Ok(Box::new(CampaignModule::new(
			base,
			roles,
			finish,
			interactions,
			route_gate,
			chapter_index,
		)))
	}
}

pub(super) struct ChapteredFactory(ChapterRun);

impl ChapteredFactory {
	pub(super) fn new(settings: &InteractionSettings) -> Option<Self> {
		Some(Self(ChapterRun::new(
			Arc::new(settings.campaign.clone()?),
			settings.route_gate.as_ref().map(|route| &route.scene_id),
		)))
	}
}

impl GameModuleFactory for ChapteredFactory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(CampaignProgress {
			chapters: Some(self.0.clone()),
			..CampaignProgress::default()
		})
	}

	fn prefab_references(
		&self,
		scene_settings: &serde_json::Value,
	) -> Result<engine_core::simulation_session::ModuleSettingReferences, String> {
		CampaignFactory.prefab_references(scene_settings)
	}

	fn create(
		&self,
		project: &ProjectData,
		scene: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		CampaignFactory.create(project, scene)
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		CampaignFactory.create_prepared(config)
	}
}

fn validate_route_gate(
	config: &PreparedModuleConfig<'_>,
	route: &RouteGateSettings,
	roles: &CourseRoles,
	key_trigger: &EntityKey,
	exit_trigger: &EntityKey,
	exit_blocker: Option<&EntityKey>,
) -> Result<(), String> {
	let reserved = [
		&roles.player,
		&roles.key,
		&roles.exit,
		key_trigger,
		exit_trigger,
	];
	if reserved.contains(&&route.lever)
		|| reserved.contains(&&route.gate)
		|| exit_blocker.is_some_and(|blocker| blocker == &route.lever || blocker == &route.gate)
	{
		return Err(
			"The route lever and gate must be separate from the course roles and sensors".into(),
		);
	}
	let find = |key: &EntityKey| {
		config
			.document()
			.entities
			.iter()
			.find(|entity| &entity.key == key)
	};
	let lever = find(&route.lever).ok_or("Route lever references a missing entity")?;
	if lever
		.interactable
		.as_ref()
		.is_some_and(|component| component.enabled && component.settings.action != route.action)
	{
		return Err(
			"Route lever Interactable action must match the bound Route Gate action".into(),
		);
	}
	let gate = find(&route.gate).ok_or("Route gate references a missing entity")?;
	let interaction = gate
		.collider
		.as_ref()
		.ok_or("Route gate needs a solid collider")?
		.interaction();
	let player = find(&roles.player)
		.and_then(|entity| entity.collider.as_ref())
		.ok_or("The Player needs a collider to use the route lever")?
		.interaction();
	if interaction.sensor || !interaction.detects(player) {
		return Err("Route gate needs a blocking collider that interacts with the Player".into());
	}
	// A restored open gate keeps the same authored collider definition.
	if gate.collision_enabled != gate.renderable {
		return Err("Route gate visibility and collision must agree".into());
	}
	Ok(())
}

pub(super) struct FinishRoles {
	pub(super) player: EntityKey,
	pub(super) marker: EntityKey,
}

impl FinishRoles {
	fn from_config(config: &PreparedModuleConfig<'_>) -> Result<Self, String> {
		let bindings = &config.document().bindings;

		let required = |name: &str| {
			bindings.get(name).cloned().ok_or_else(|| {
				format!(
					"Scene {:?} requires a {name:?} binding",
					config.scene_id().as_str(),
				)
			})
		};

		let player = required("player")?;
		let marker = required("finish")?;

		if player == marker {
			return Err("Player and Finish must reference different entities".into());
		}

		let player_can_move = can_move(config, &player);

		if !player_can_move {
			return Err("The finish scene's Player needs a controlled movement \
                 behavior with positive speed"
				.into());
		}

		Ok(Self { player, marker })
	}
}

pub(super) struct CourseRoles {
	pub(super) player: EntityKey,
	pub(super) key: EntityKey,
	pub(super) exit: EntityKey,
	pub(super) destination: SceneId,
}

impl CourseRoles {
	fn from_config(
		config: &PreparedModuleConfig<'_>,
		destination: Option<&SceneId>,
	) -> Result<Option<Self>, String> {
		let bindings = &config.document().bindings;

		if !bindings.contains_key("key") && !bindings.contains_key("exit") {
			return Ok(None);
		}

		let required = |name: &str| {
			bindings.get(name).cloned().ok_or_else(|| {
				format!(
					"Scene {:?} requires a {name:?} binding for Courtyard Run",
					config.scene_id().as_str(),
				)
			})
		};

		let player = required("player")?;
		let key = required("key")?;
		let exit = required("exit")?;

		if player == key || player == exit || key == exit {
			return Err("Player, Key, and Exit must reference different entities".into());
		}

		let player_can_move = can_move(config, &player);

		if !player_can_move {
			return Err("The Player role needs a controlled movement behavior \
                 with positive speed"
				.into());
		}

		let destination = destination
			.cloned()
			.unwrap_or(SceneId::new(FINISH_SCENE_ID)?);
		config.project().scenes.get(&destination).map_err(|_| {
			format!(
				"Scene {:?} requires the Courtyard Run destination scene {:?} in the project catalog",
				config.scene_id().as_str(),
				destination.as_str(),
			)
		})?;

		Ok(Some(Self {
			player,
			key,
			exit,
			destination,
		}))
	}
}

fn can_move(config: &PreparedModuleConfig<'_>, player: &EntityKey) -> bool {
	config.behaviors().iter().any(|behavior| {
		behavior.entity == *player
			&& matches!(
				&behavior.settings,
				BehaviorSettings::ControlledMove { speed, .. }
					if *speed > 0.0
			)
	})
}
