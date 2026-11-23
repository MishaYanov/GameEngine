//! A compiled trigger-driven door with an authored opening angle and speed.

use engine_core::components::{
	ScriptDescriptor, ScriptField, ScriptProperty, ScriptReferenceField, ScriptReferenceKind,
	ScriptSettings,
};
use engine_core::{
	simulation_session::{
		ObjectScript, ObjectScriptCommands, ObjectScriptContext, ObjectScriptTrigger,
	},
	triggers::TriggerPhase,
};

#[derive(Clone)]
struct OpenDoorOnTrigger {
	angle: f32,
	speed: f32,
	closed: f32,
	opening: bool,
	opened: bool,
}

impl ObjectScript for OpenDoorOnTrigger {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}

	fn needs_update(&self) -> bool {
		self.opening
	}

	fn on_start(
		&mut self,
		context: &ObjectScriptContext<'_>,
		_: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.closed = context.transform(context.owner())?.rotation[1];
		Ok(())
	}

	fn on_trigger(
		&mut self,
		context: &ObjectScriptContext<'_>,
		event: &ObjectScriptTrigger,
		_: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		if !self.opened
			&& event.phase == TriggerPhase::Enter
			&& context.reference("sensor") == Some(&event.sensor)
		{
			self.opening = true;
		}
		Ok(())
	}

	fn on_fixed_update(
		&mut self,
		context: &ObjectScriptContext<'_>,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		let yaw = context.transform(context.owner())?.rotation[1];
		let goal = self.closed + self.angle;
		let difference = goal - yaw;
		let advance = self.speed * context.step_seconds();
		if difference.abs() <= advance {
			if yaw != goal {
				commands.set_local_yaw(goal)?;
			}
			self.opened = true;
			self.opening = false;
		} else {
			commands.set_local_yaw(yaw + difference.signum() * advance)?;
		}
		Ok(())
	}
}

pub(crate) fn instantiate(settings: &ScriptSettings) -> Result<Box<dyn ObjectScript>, String> {
	let descriptor = descriptor();
	let radians = |key| match descriptor.property(settings, key) {
		Some(ScriptProperty::Number(value)) => Ok(value.to_radians()),
		_ => Err(format!("Open Door requires numeric {key}")),
	};
	Ok(Box::new(OpenDoorOnTrigger {
		angle: radians("open_angle_degrees")?,
		speed: radians("speed_degrees_per_second")?,
		closed: 0.,
		opening: false,
		opened: false,
	}))
}

pub(crate) fn descriptor() -> ScriptDescriptor {
	ScriptDescriptor {
		id: "project.open_door_on_trigger".into(),
		label: "Open Door on Trigger".into(),
		config_version: 1,
		fields: vec![
			ScriptField::Number {
				key: "open_angle_degrees".into(),
				label: "Open angle (degrees)".into(),
				default: 90.0,
				min: 0.0,
				max: 180.0,
			},
			ScriptField::Number {
				key: "speed_degrees_per_second".into(),
				label: "Speed (degrees per second)".into(),
				default: 180.0,
				min: 1.0,
				max: 720.0,
			},
		],
		references: vec![ScriptReferenceField {
			key: "sensor".into(),
			label: "Trigger sensor".into(),
			requires: ScriptReferenceKind::TriggerCollider,
		}],
		receive_local_actions: false,
		write_owner_transform: true,
	}
}
