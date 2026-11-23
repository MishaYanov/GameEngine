use super::{ApprovedSource, Compatibility, CorpusError, CorpusManifest, SearchScope};
use crate::CorpusId;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, io, mem};

const MAX_CHUNK_BYTES: usize = 1800;
const MAX_HEADING_BYTES: usize = 160;
const MAX_CHUNKS_PER_PARTITION: usize = 1024;
const MAX_PARTITIONS: usize = 32;
const SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
	fn of(bytes: &[u8]) -> Self {
		Self(Sha256::digest(bytes).into())
	}
}

impl fmt::Display for ContentHash {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		for byte in self.0 {
			write!(formatter, "{byte:02x}")?;
		}
		Ok(())
	}
}

impl Serialize for ContentHash {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.collect_str(self)
	}
}

/// A relative, versioned reference. Project citations carry their exact project ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Citation {
	pub corpus: CorpusId,
	pub source_id: String,
	pub relative_path: String,
	pub heading: String,
	pub ordinal: usize,
	pub source_revision: ContentHash,
	pub content_hash: ContentHash,
	pub compatibility: Compatibility,
	pub project_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KnowledgeChunk {
	pub citation: Citation,
	pub text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefreshStats {
	pub added: usize,
	pub updated: usize,
	pub removed: usize,
}

#[derive(Default)]
pub struct KnowledgeIndex {
	partitions: BTreeMap<Partition, Sources>,
}

impl KnowledgeIndex {
	/// Replace the approved partitions after loading all sources successfully.
	/// Deleted sources are dropped. Other projects and compatibility versions remain isolated.
	pub fn refresh(&mut self, manifest: &CorpusManifest) -> Result<RefreshStats, CorpusError> {
		let [engine_key, project_key] = keys(manifest.scope());
		let mut engine = Sources::default();
		let mut project = Sources::default();
		for source in &manifest.sources {
			let Some(text) = manifest.read(source)? else {
				continue;
			};
			let (sources, project_id) = match source.corpus {
				CorpusId::EngineHelp => (&mut engine, None),
				CorpusId::ActiveProject => (&mut project, manifest.scope().project_id.as_deref()),
			};
			let document =
				chunk_document(source, &text, &manifest.scope().compatibility, project_id)?;
			sources.insert(source.id.clone(), document)?;
		}
		let candidates = [engine_key.zip(Some(engine)), project_key.zip(Some(project))];
		let new_partitions = candidates
			.iter()
			.flatten()
			.filter(|(key, value)| {
				!value.documents.is_empty() && !self.partitions.contains_key(key)
			})
			.count();
		let removed_partitions = candidates
			.iter()
			.flatten()
			.filter(|(key, value)| value.documents.is_empty() && self.partitions.contains_key(key))
			.count();
		if self.partitions.len() + new_partitions - removed_partitions > MAX_PARTITIONS {
			return Err(CorpusError::Budget("index partitions"));
		}

		let mut stats = RefreshStats::default();
		for (key, sources) in candidates.into_iter().flatten() {
			let previous = self.partitions.remove(&key).unwrap_or_default();
			for (id, source) in &sources.documents {
				match previous.documents.get(id) {
					None => stats.added += 1,
					Some(old) if old != source => stats.updated += 1,
					Some(_) => {}
				}
			}
			stats.removed += previous
				.documents
				.keys()
				.filter(|id| !sources.documents.contains_key(*id))
				.count();
			if !sources.documents.is_empty() {
				self.partitions.insert(key, sources);
			}
		}
		Ok(stats)
	}

	pub fn chunks(&self, scope: &SearchScope) -> impl Iterator<Item = &KnowledgeChunk> {
		keys(scope)
			.into_iter()
			.flatten()
			.filter_map(|key| self.partitions.get(&key))
			.flat_map(|partition| partition.documents.values())
			.flat_map(|source| &source.chunks)
	}

	/// Serialize only a compatible engine and authorized project partition.
	/// The host controls whether and where this snapshot is stored.
	pub fn write_snapshot(
		&self,
		scope: &SearchScope,
		writer: impl io::Write,
	) -> serde_json::Result<()> {
		serde_json::to_writer(
			writer,
			&Snapshot {
				version: SNAPSHOT_VERSION,
				compatibility: &scope.compatibility,
				project_id: scope.project_id.as_deref(),
				chunks: self.chunks(scope).collect(),
			},
		)
	}
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Partition {
	compatibility: Compatibility,
	project_id: Option<String>,
}

fn keys(scope: &SearchScope) -> [Option<Partition>; 2] {
	[
		Some(Partition {
			compatibility: scope.compatibility.clone(),
			project_id: None,
		}),
		scope.project_id.as_ref().map(|id| Partition {
			compatibility: scope.compatibility.clone(),
			project_id: Some(id.clone()),
		}),
	]
}

#[derive(Default)]
struct Sources {
	documents: BTreeMap<String, SourceDocument>,
	chunks: usize,
}

impl Sources {
	fn insert(&mut self, id: String, source: SourceDocument) -> Result<(), CorpusError> {
		self.chunks += source.chunks.len();
		if self.chunks > MAX_CHUNKS_PER_PARTITION {
			return Err(CorpusError::Budget("partition chunks"));
		}
		self.documents.insert(id, source);
		Ok(())
	}
}

#[derive(Eq, PartialEq)]
struct SourceDocument {
	revision: ContentHash,
	chunks: Vec<KnowledgeChunk>,
}

#[derive(Serialize)]
struct Snapshot<'a> {
	version: u32,
	compatibility: &'a Compatibility,
	project_id: Option<&'a str>,
	chunks: Vec<&'a KnowledgeChunk>,
}

fn chunk_document(
	source: &ApprovedSource,
	text: &str,
	compatibility: &Compatibility,
	project_id: Option<&str>,
) -> Result<SourceDocument, CorpusError> {
	let revision = ContentHash::of(text.as_bytes());
	let mut document = SourceDocument {
		revision,
		chunks: Vec::new(),
	};
	let mut section = source.id.as_str();
	let mut fence = None;
	let mut body = String::new();
	let mut flush = |body: &mut String, heading: &str| -> Result<(), CorpusError> {
		let text = mem::take(body);
		let text = text.trim();
		if text.is_empty() {
			return Ok(());
		}
		if document.chunks.len() == MAX_CHUNKS_PER_PARTITION {
			return Err(CorpusError::Budget("source chunks"));
		}
		document.chunks.push(KnowledgeChunk {
			citation: Citation {
				corpus: source.corpus,
				source_id: source.id.clone(),
				relative_path: source.display_path.clone(),
				heading: heading[..floor_char_boundary(heading, MAX_HEADING_BYTES)].to_owned(),
				ordinal: document.chunks.len(),
				source_revision: revision,
				content_hash: ContentHash::of(text.as_bytes()),
				compatibility: compatibility.clone(),
				project_id: project_id.map(str::to_owned),
			},
			text: text.to_owned(),
		});
		Ok(())
	};

	for mut line in text.split_inclusive('\n') {
		if source.is_markdown() {
			let trimmed = line.trim();
			if let Some(marker) = fence_marker(trimmed) {
				match fence {
					Some(open) if open == marker => fence = None,
					None => fence = Some(marker),
					_ => {}
				}
			} else if fence.is_none()
				&& let Some(heading) = markdown_heading(trimmed)
			{
				flush(&mut body, section)?;
				section = heading;
				continue;
			}
		}
		if body.len() + line.len() > MAX_CHUNK_BYTES {
			flush(&mut body, section)?;
		}
		while line.len() > MAX_CHUNK_BYTES {
			let (head, tail) = line.split_at(floor_char_boundary(line, MAX_CHUNK_BYTES));
			body.push_str(head);
			flush(&mut body, section)?;
			line = tail;
		}
		body.push_str(line);
	}
	flush(&mut body, section)?;
	Ok(document)
}

fn fence_marker(line: &str) -> Option<u8> {
	["```", "~~~"]
		.iter()
		.find(|marker| line.starts_with(**marker))
		.map(|marker| marker.as_bytes()[0])
}

fn floor_char_boundary(text: &str, limit: usize) -> usize {
	let mut end = text.len().min(limit);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	end
}

fn markdown_heading(line: &str) -> Option<&str> {
	let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
	if !(1..=6).contains(&hashes) || !line[hashes..].starts_with(char::is_whitespace) {
		return None;
	}
	let heading = line[hashes..].trim();
	let without_closing_hashes = heading.trim_end_matches('#');
	let heading = if without_closing_hashes.ends_with(char::is_whitespace) {
		without_closing_hashes.trim_end()
	} else {
		heading
	};
	(!heading.is_empty()).then_some(heading)
}
