use runtime::{
    InputState, Key,
    scene::{EntityId, Scene},
    scene_editor::movement_direction,
};

pub(crate) struct RobotGame {
    left_robot: EntityId,
    right_robot: EntityId,
    animation_paused: bool,
}

impl RobotGame {
    pub(crate) fn from_scene(scene: &Scene) -> Result<Self, String> {
        let left_robot = scene
            .binding("robot_a")
            .ok_or("The robot demo requires binding \"robot_a\"")?;
        let right_robot = scene
            .binding("robot_b")
            .ok_or("The robot demo requires binding \"robot_b\"")?;
        if left_robot == right_robot {
            return Err("The robot demo requires two distinct entities".to_string());
        }
        Ok(Self {
            left_robot,
            right_robot,
            animation_paused: false,
        })
    }

    pub(crate) fn animation_paused(&self) -> bool {
        self.animation_paused
    }

    pub(crate) fn update(
        &mut self,
        scene: &mut Scene,
        input: &InputState,
        delta_seconds: f32,
    ) -> Result<(), String> {
        if input.just_pressed(Key::Space) {
            self.animation_paused = !self.animation_paused;
        }
        if self.animation_paused {
            return Ok(());
        }
        let direction = movement_direction(input);
        if let Some(left) = scene.get_mut(self.left_robot) {
            let mut transform = *left.transform();
            transform.rotation[0] += 0.6 * delta_seconds;
            transform.rotation[1] += 0.9 * delta_seconds;
            transform.position[0] += direction[0] * delta_seconds;
            transform.position[1] += direction[1] * delta_seconds;
            transform.validate()?;
            *left.transform_mut() = transform;
        }
        if let Some(right) = scene.get_mut(self.right_robot) {
            let mut transform = *right.transform();
            transform.rotation[0] += delta_seconds * -0.5;
            transform.rotation[1] += delta_seconds * 0.75;
            transform.rotation[2] += delta_seconds * 0.2;
            transform.validate()?;
            *right.transform_mut() = transform;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests_game.rs"]
mod tests;
