use super::{ContentHash, KnowledgeChunk};
use std::collections::{HashMap, VecDeque};

const MAX_DIMENSIONS: usize = 2048;
const MAX_CACHED_CHUNKS: usize = 256;
const MAX_MODEL_TAG_BYTES: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingModel {
	pub id: String,
	pub revision: String,
	pub dimensions: usize,
}

impl EmbeddingModel {
	pub fn new(id: impl Into<String>, revision: impl Into<String>, dimensions: usize) -> Self {
		Self {
			id: id.into(),
			revision: revision.into(),
			dimensions,
		}
	}

	fn is_supported(&self) -> bool {
		(1..=MAX_DIMENSIONS).contains(&self.dimensions)
			&& [&self.id, &self.revision].into_iter().all(|tag| {
				!tag.trim().is_empty()
					&& tag.len() <= MAX_MODEL_TAG_BYTES
					&& !tag.chars().any(char::is_control)
			})
	}
}

/// A host-supplied local model. A failed or unavailable model leaves keyword retrieval usable.
pub trait EmbeddingProvider {
	fn model(&self) -> EmbeddingModel;
	fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
}

/// Bounded FIFO cache keyed by the active model identity and chunk digest.
#[derive(Default)]
pub struct EmbeddingCache {
	model: Option<EmbeddingModel>,
	vectors: HashMap<ContentHash, Vec<f32>>,
	insertion_order: VecDeque<ContentHash>,
}

impl EmbeddingCache {
	pub fn cached_count(&self) -> usize {
		self.vectors.len()
	}

	pub(super) fn query_vector(
		&mut self,
		provider: &dyn EmbeddingProvider,
		query: &str,
	) -> Option<Vec<f32>> {
		let model = provider.model();
		if self.model.as_ref() != Some(&model) {
			self.vectors.clear();
			self.insertion_order.clear();
			self.model = None;
			if !model.is_supported() {
				return None;
			}
			self.model = Some(model);
		}
		normalized(provider.embed(query).ok()?, self.model.as_ref()?.dimensions)
	}

	pub(super) fn chunk_vector(
		&mut self,
		provider: &dyn EmbeddingProvider,
		chunk: &KnowledgeChunk,
	) -> Option<&[f32]> {
		let hash = chunk.citation.content_hash;
		if !self.vectors.contains_key(&hash) {
			let values = normalized(
				provider.embed(&chunk.text).ok()?,
				self.model.as_ref()?.dimensions,
			)?;
			while self.vectors.len() >= MAX_CACHED_CHUNKS {
				let oldest = self.insertion_order.pop_front()?;
				self.vectors.remove(&oldest);
			}
			self.insertion_order.push_back(hash);
			self.vectors.insert(hash, values);
		}
		self.vectors.get(&hash).map(Vec::as_slice)
	}
}

fn normalized(mut values: Vec<f32>, dimensions: usize) -> Option<Vec<f32>> {
	if values.len() != dimensions || values.iter().any(|value| !value.is_finite()) {
		return None;
	}
	let sum_sq: f64 = values.iter().map(|&value| f64::from(value).powi(2)).sum();
	if sum_sq == 0.0 {
		return None;
	}
	let length = sum_sq.sqrt();
	for value in &mut values {
		*value = (f64::from(*value) / length) as f32;
	}
	Some(values)
}
