use crate::assets::Model;
use crate::Transform;
use std::sync::Arc;

pub struct ModelInstance {
    model: Arc<Model>,
    transform: Transform,
}

impl ModelInstance {
    pub fn new(model: Arc<Model>, transform: Transform) -> Self {
        Self { model, transform }
    }

    pub fn model(&self) -> &Model {
        self.model.as_ref()
    }

    pub fn transform(&self) -> &Transform {
        &self.transform
    }

    pub fn transform_mut(&mut self) -> &mut Transform {
        &mut self.transform
    }
}
