use runtime::{
    InputState, Key,
    scene::{EntityId, Scene},
};

pub(crate) struct RobotGame {
    left_robot: EntityId,
    right_robot: EntityId,
    selected_robot: EntityId,
}

impl RobotGame {
    pub(crate) fn from_entities(ids: &[EntityId]) -> Result<Self, String> {
        // Version 1 of this demo assigns gameplay roles in scene-file order.
        // Names remain labels, so renaming an entity does not change its role.
        let [left_robot, right_robot, ..] = ids else {
            return Err("The robot demo requires at least two scene entities".to_string());
        };
        if left_robot == right_robot {
            return Err("The robot demo requires two distinct entities".to_string());
        }

        Ok(Self {
            left_robot: *left_robot,
            right_robot: *right_robot,
            selected_robot: *left_robot,
        })
    }

    pub(crate) fn update(&mut self, scene: &mut Scene, input: &InputState, delta_seconds: f32) {
        if input.just_pressed(Key::Tab) {
            self.selected_robot = if self.selected_robot == self.left_robot {
                self.right_robot
            } else {
                self.left_robot
            };
        }

        let mut direction = [0.0_f32; 2];

        if input.is_held(Key::W) {
            direction[1] += 1.0;
        }

        if input.is_held(Key::S) {
            direction[1] -= 1.0;
        }

        if input.is_held(Key::A) {
            direction[0] -= 1.0;
        }

        if input.is_held(Key::D) {
            direction[0] += 1.0;
        }

        let length = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();

        if length > 0.0 {
            direction[0] /= length;
            direction[1] /= length;
        }
        let move_speed = 1.0; // World units per second.
        let distance = move_speed * delta_seconds;

        if let Some(left) = scene.get_mut(self.left_robot) {
            let transform = left.transform_mut();

            transform.rotation[0] += 0.6 * delta_seconds;
            transform.rotation[1] += 0.9 * delta_seconds;
        }

        if let Some(right) = scene.get_mut(self.right_robot) {
            let rotation = &mut right.transform_mut().rotation;
            rotation[0] += delta_seconds * -0.5;
            rotation[1] += delta_seconds * 0.75;
            rotation[2] += delta_seconds * 0.2;
        }

        if let Some(selected) = scene.get_mut(self.selected_robot) {
            let transform = selected.transform_mut();

            transform.position[0] += direction[0] * distance;
            transform.position[1] += direction[1] * distance;
        }
    }
}

#[cfg(test)]
#[path = "tests_game.rs"]
mod tests;
