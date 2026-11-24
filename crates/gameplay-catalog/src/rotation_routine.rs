//! Bounded scene-authored routines for an object's local rotation.

use engine_core::{
	components::{ScriptDescriptor, ScriptField, ScriptProperty, ScriptSettings},
	simulation_session::{
		ObjectScript, ObjectScriptCommands, ObjectScriptContext, ObjectScriptRegistration,
	},
};
use serde_json::Value;

const MAX_PROGRAM_BYTES: usize = 256;
const MAX_RULES: usize = 6;
const DEFAULT_PROGRAM: &str = r#"[{"during":"fixed_update","degrees":30}]"#;

#[derive(Clone, Debug, PartialEq)]
enum Hook {
	Start,
	FixedUpdate,
}

#[derive(Clone)]
struct Rule {
	hook: Hook,
	degrees: f32,
}

#[derive(Clone)]
struct RotationRoutine {
	rules: Vec<Rule>,
	last_yaw: Option<(u64, f32)>,
}

impl RotationRoutine {
	fn run(
		&mut self,
		hook: Hook,
		context: &ObjectScriptContext<'_>,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		let mut rules = self
			.rules
			.iter()
			.filter(|rule| rule.hook == hook)
			.peekable();
		if rules.peek().is_none() {
			return Ok(());
		}
		let tick = context.tick();
		let mut yaw = match self.last_yaw {
			Some((previous_tick, yaw)) if previous_tick == tick => yaw,
			_ => context.transform(context.owner())?.rotation[1],
		};
		for rule in rules {
			let amount = rule.degrees.to_radians();
			yaw = match hook {
				Hook::Start => amount,
				Hook::FixedUpdate => yaw + amount * context.step_seconds(),
			};
		}
		let yaw =
			(yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
		commands.set_local_yaw(yaw)?;
		self.last_yaw = Some((tick, yaw));
		Ok(())
	}
}

impl ObjectScript for RotationRoutine {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}

	fn needs_update(&self) -> bool {
		self.rules.iter().any(|rule| rule.hook == Hook::FixedUpdate)
	}

	fn on_start(
		&mut self,
		context: &ObjectScriptContext<'_>,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.run(Hook::Start, context, commands)
	}

	fn on_fixed_update(
		&mut self,
		context: &ObjectScriptContext<'_>,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.run(Hook::FixedUpdate, context, commands)
	}
}

fn parse_rule(value: &Value) -> Result<Rule, &'static str> {
	let rule = value.as_object().ok_or("Expected a rule object")?;
	if rule
		.keys()
		.any(|field| !matches!(field.as_str(), "during" | "degrees"))
	{
		return Err("Unexpected rule field");
	}
	let degrees = rule
		.get("degrees")
		.and_then(Value::as_f64)
		.filter(|value| (-720.0..=720.0).contains(value))
		.ok_or("Degrees must be a number between -720 and 720")? as f32;
	let hook = match rule.get("during").and_then(Value::as_str) {
		Some("start") => Hook::Start,
		Some("fixed_update") => Hook::FixedUpdate,
		_ => return Err("Expected the start or fixed_update hook"),
	};
	Ok(Rule { hook, degrees })
}

fn instantiate(settings: &ScriptSettings) -> Result<Box<dyn ObjectScript>, String> {
	let Some(ScriptProperty::Text(source)) = descriptor().property(settings, "routine") else {
		return Err("The routine must be JSON text".into());
	};
	if source.len() > MAX_PROGRAM_BYTES {
		return Err(format!("The routine exceeds {MAX_PROGRAM_BYTES} bytes"));
	}
	let rules: Vec<Value> = serde_json::from_str(&source)
		.map_err(|cause| format!("Invalid routine JSON at column {}", cause.column()))?;
	if rules.is_empty() || rules.len() > MAX_RULES {
		return Err(format!(
			"The routine must contain between 1 and {MAX_RULES} rules"
		));
	}
	let rules = rules
		.iter()
		.enumerate()
		.map(|(index, value)| {
			parse_rule(value).map_err(|error| format!("Routine rule {}: {error}", index + 1))
		})
		.collect::<Result<Vec<_>, _>>()?;
	Ok(Box::new(RotationRoutine {
		rules,
		last_yaw: None,
	}))
}

fn descriptor() -> ScriptDescriptor {
	ScriptDescriptor {
		id: "engine.rotation_routine".into(),
		label: "Rotation Routine".into(),
		config_version: 1,
		fields: vec![ScriptField::Text {
			key: "routine".into(),
			label: "Routine JSON".into(),
			default: DEFAULT_PROGRAM.into(),
			max_bytes: MAX_PROGRAM_BYTES,
		}],
		references: Vec::new(),
		receive_local_actions: false,
		write_owner_transform: true,
	}
}

pub(crate) const fn registration() -> ObjectScriptRegistration {
	ObjectScriptRegistration {
		descriptor,
		instantiate,
	}
}
