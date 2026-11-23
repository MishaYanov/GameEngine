//! Versioned, read-only requests for a separate authoring conversation worker.

use crate::knowledge::ReferenceHit;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
	io::{self, BufRead},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
		mpsc,
	},
	thread::{self, JoinHandle},
};

pub const WORKER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_WORKER_LINE_BYTES: usize = 128 * 1024;
pub const MAX_REQUEST_ID_BYTES: usize = 64;
const MAX_QUESTION_CHARACTERS: usize = 512;
const MAX_COMPONENTS: usize = 16;
const MAX_COMPONENT_BYTES: usize = 8 * 1024;
const MAX_ANSWER_BYTES: usize = 32 * 1024;
const MAX_DELTA_BYTES: usize = 2 * 1024;
pub const MAX_CITATIONS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestScope {
	pub project_id: String,
	pub workspace_generation: u64,
	pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerScope {
	project_id: String,
	workspace_generation: u64,
}

impl WorkerScope {
	pub fn new(project_id: String, workspace_generation: u64) -> Option<Self> {
		valid_token(&project_id, 128).then_some(Self {
			project_id,
			workspace_generation,
		})
	}

	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	fn matches(&self, scope: &RequestScope) -> bool {
		self.project_id == scope.project_id
			&& self.workspace_generation == scope.workspace_generation
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
	tag = "kind",
	rename_all = "snake_case",
	rename_all_fields = "camelCase",
	deny_unknown_fields
)]
pub enum SubjectAddress {
	Scene {
		scene: String,
		object: String,
	},
	Prop {
		draft_id: String,
		source_object: String,
	},
	Linked {
		scene: String,
		root: String,
		address: LinkedAddress,
	},
}

impl SubjectAddress {
	fn valid(&self) -> bool {
		match self {
			Self::Scene { scene, object } => valid_key(scene) && valid_key(object),
			Self::Prop {
				draft_id,
				source_object,
			} => valid_token(draft_id, 256) && valid_key(source_object),
			Self::Linked {
				scene,
				root,
				address,
			} => valid_key(scene) && valid_key(root) && address.valid(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkedAddress {
	Object {
		placement_path: Vec<String>,
		source_entity: String,
	},
	Placement {
		placement_path: Vec<String>,
	},
}

impl LinkedAddress {
	fn valid(&self) -> bool {
		let path = match self {
			Self::Object {
				placement_path,
				source_entity,
			} => {
				if !valid_key(source_entity) {
					return false;
				}
				placement_path
			}
			Self::Placement { placement_path } => placement_path,
		};
		path.len() <= 8 && path.iter().all(|key| valid_token(key, 128))
	}
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredTransform {
	pub position: [f32; 3],
	pub rotation: [f32; 3],
	pub scale: [f32; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionCapability {
	InspectObject,
	InspectComponent,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSummary {
	pub id: String,
	pub type_id: String,
	pub schema_version: u32,
	pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedComponent {
	pub id: String,
	pub schema_version: u32,
	pub enabled: bool,
	/// Typed by the native component service before transfer to this model-only worker.
	pub data: Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionSnapshot {
	pub workspace_generation: u64,
	pub revision: u64,
	pub content_revision: u64,
	pub address: SubjectAddress,
	pub entity_key: String,
	pub name: String,
	pub transform: AuthoredTransform,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub parent: Option<String>,
	pub components: Vec<ComponentSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub selected_component: Option<SelectedComponent>,
	pub capabilities: Vec<InspectionCapability>,
}

impl SelectionSnapshot {
	fn valid_for(&self, scope: &RequestScope) -> bool {
		self.workspace_generation == scope.workspace_generation
			&& self.revision == scope.revision
			&& self.address.valid()
			&& valid_key(&self.entity_key)
			&& self.parent.as_deref().is_none_or(valid_key)
			&& self.name.chars().count() <= 128
			&& self.components.len() <= MAX_COMPONENTS
			&& self.capabilities.len() <= 2
			&& self.components.iter().all(|component| {
				valid_token(&component.id, 128) && valid_token(&component.type_id, 128)
			}) && self.selected_component.as_ref().is_none_or(|selected| {
			self.components
				.iter()
				.any(|summary| summary.id == selected.id)
				&& selected.data.is_object()
				&& serde_json::to_vec(selected)
					.is_ok_and(|encoded| encoded.len() <= MAX_COMPONENT_BYTES)
		}) && self
			.transform
			.position
			.iter()
			.chain(&self.transform.rotation)
			.chain(&self.transform.scale)
			.all(|coordinate| coordinate.is_finite())
	}
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRequest {
	pub id: String,
	pub scope: RequestScope,
	pub question: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub selection: Option<SelectionSnapshot>,
}

impl AgentRequest {
	fn error(&self, scope: &WorkerScope) -> Option<&'static str> {
		if !scope.matches(&self.scope) {
			return Some("This request belongs to a different project workspace");
		}
		if self.question.trim().is_empty()
			|| self.question.chars().count() > MAX_QUESTION_CHARACTERS
		{
			return Some("Enter a question between 1 and 512 characters");
		}
		if self
			.selection
			.as_ref()
			.is_some_and(|selection| !selection.valid_for(&self.scope))
		{
			return Some("The authored selection no longer matches the request");
		}
		None
	}
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerInput {
	Start { request: Box<AgentRequest> },
	Stop { id: String },
}

#[derive(Debug, Serialize)]
pub struct WorkerEvent {
	pub id: String,
	#[serde(flatten)]
	pub kind: WorkerEventKind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEventKind {
	Started,
	Sources {
		citations: Vec<ReferenceHit>,
	},
	Delta {
		text: String,
	},
	Finished {
		status: FinishStatus,
		#[serde(skip_serializing_if = "Option::is_none")]
		message: Option<&'static str>,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishStatus {
	Completed,
	RetrievalOnly,
	Cancelled,
	Error,
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
	pub fn cancel(&self) {
		self.0.store(true, Ordering::Release);
	}

	pub fn is_cancelled(&self) -> bool {
		self.0.load(Ordering::Acquire)
	}
}

pub struct ModelInput<'a> {
	pub question: &'a str,
	pub selection: Option<&'a SelectionSnapshot>,
	pub references: &'a [ReferenceHit],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelError {
	Unavailable,
	Interrupted,
}

/// A provider cooperatively checks cancellation and stops when the emitter returns false.
pub trait ModelProvider: Send + 'static {
	fn generate(
		&mut self,
		input: ModelInput<'_>,
		cancel: &CancellationToken,
		emit: &mut dyn FnMut(&str) -> bool,
	) -> Result<(), ModelError>;
}

/// Returns references only for the trusted worker scope established at launch.
pub trait ReferenceProvider: Send + 'static {
	fn search(&mut self, question: &str, cancel: &CancellationToken) -> Vec<ReferenceHit>;
}

type EventSink = dyn Fn(WorkerEvent) + Send + Sync;

struct Job {
	request: Box<AgentRequest>,
	cancellation: CancellationToken,
}

struct ActiveRequest {
	id: String,
	cancellation: CancellationToken,
}

/// One generation per worker and at most one running request.
pub struct Session {
	scope: WorkerScope,
	active: Arc<Mutex<Option<ActiveRequest>>>,
	send: Arc<EventSink>,
	jobs: Option<mpsc::SyncSender<Job>>,
	thread: Option<JoinHandle<()>>,
}

impl Session {
	pub fn new(
		scope: WorkerScope,
		mut references: impl ReferenceProvider,
		mut model: Option<Box<dyn ModelProvider>>,
		send: impl Fn(WorkerEvent) + Send + Sync + 'static,
	) -> io::Result<Self> {
		let send: Arc<EventSink> = Arc::new(send);
		let active = Arc::new(Mutex::new(None::<ActiveRequest>));
		let (jobs, receiver) = mpsc::sync_channel::<Job>(1);
		let worker_send = Arc::clone(&send);
		let worker_active = Arc::clone(&active);
		let thread = thread::Builder::new()
			.name("agent-answer".into())
			.spawn(move || {
				while let Ok(job) = receiver.recv() {
					let (status, message) =
						answer(&job, &mut references, &mut model, &*worker_send);
					if let Ok(mut active) = worker_active.lock() {
						let status = if job.cancellation.is_cancelled() {
							FinishStatus::Cancelled
						} else {
							status
						};
						emit(
							&*worker_send,
							&job.request.id,
							WorkerEventKind::Finished {
								status,
								message: if status == FinishStatus::Cancelled {
									None
								} else {
									message
								},
							},
						);
						*active = None;
					}
				}
			})?;
		Ok(Self {
			scope,
			active,
			send,
			jobs: Some(jobs),
			thread: Some(thread),
		})
	}

	/// Rejects invalid and duplicate submissions without interrupting the active request.
	pub fn handle(&self, input: WorkerInput) {
		match input {
			WorkerInput::Start { request } => self.start(request),
			WorkerInput::Stop { id } => self.cancel(Some(&id)),
		}
	}

	fn start(&self, request: Box<AgentRequest>) {
		if !valid_token(&request.id, MAX_REQUEST_ID_BYTES) {
			return;
		}
		let Ok(mut active) = self.active.lock() else {
			self.reject(&request.id, "The Agent worker is unavailable");
			return;
		};
		if active.as_ref().is_some_and(|job| job.id == request.id) {
			return;
		}
		if let Some(error) = request.error(&self.scope) {
			self.reject(&request.id, error);
			return;
		}
		if active.is_some() {
			self.reject(
				&request.id,
				"Stop the active answer before starting another",
			);
			return;
		}

		let cancellation = CancellationToken::default();
		let id = request.id.clone();
		let handoff = self.jobs.as_ref().map(|jobs| {
			jobs.try_send(Job {
				request,
				cancellation: cancellation.clone(),
			})
		});
		if matches!(handoff, Some(Ok(()))) {
			*active = Some(ActiveRequest { id, cancellation });
		} else {
			self.reject(&id, "The Agent worker is unavailable");
		}
	}

	fn cancel(&self, id: Option<&str>) {
		if let Ok(active) = self.active.lock()
			&& let Some(job) = active.as_ref()
			&& id.is_none_or(|requested| requested == job.id)
		{
			job.cancellation.cancel();
		}
	}

	fn reject(&self, id: &str, message: &'static str) {
		emit(
			&*self.send,
			id,
			WorkerEventKind::Finished {
				status: FinishStatus::Error,
				message: Some(message),
			},
		);
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		self.cancel(None);
		drop(self.jobs.take());
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

fn answer(
	job: &Job,
	references: &mut impl ReferenceProvider,
	model: &mut Option<Box<dyn ModelProvider>>,
	send: &EventSink,
) -> (FinishStatus, Option<&'static str>) {
	let request = &job.request;
	let cancel = &job.cancellation;
	if cancel.is_cancelled() {
		return (FinishStatus::Cancelled, None);
	}
	emit(send, &request.id, WorkerEventKind::Started);
	let mut hits = references.search(&request.question, cancel);
	hits.truncate(MAX_CITATIONS);
	if cancel.is_cancelled() {
		return (FinishStatus::Cancelled, None);
	}
	emit(
		send,
		&request.id,
		WorkerEventKind::Sources {
			citations: hits.clone(),
		},
	);
	let Some(model) = model.as_mut() else {
		return (
			FinishStatus::RetrievalOnly,
			Some("No local model is configured"),
		);
	};

	let mut generated_bytes = 0;
	let mut reached_budget = false;
	let mut stream = |delta: &str| {
		if cancel.is_cancelled() || reached_budget {
			return false;
		}
		let allowed = MAX_ANSWER_BYTES.saturating_sub(generated_bytes);
		reached_budget = delta.len() >= allowed;
		let mut delta = &delta[..utf8_end(delta, allowed)];
		generated_bytes += delta.len();
		while !delta.is_empty() && !cancel.is_cancelled() {
			let (text, rest) = delta.split_at(utf8_end(delta, MAX_DELTA_BYTES));
			emit(
				send,
				&request.id,
				WorkerEventKind::Delta {
					text: text.to_owned(),
				},
			);
			delta = rest;
		}
		!reached_budget && !cancel.is_cancelled()
	};
	let result = model.generate(
		ModelInput {
			question: &request.question,
			selection: request.selection.as_ref(),
			references: &hits,
		},
		cancel,
		&mut stream,
	);
	if cancel.is_cancelled() {
		return (FinishStatus::Cancelled, None);
	}
	if reached_budget {
		return (
			FinishStatus::Completed,
			Some("Answer reached its size limit"),
		);
	}
	match (result, generated_bytes) {
		(Ok(()), 1..) => (FinishStatus::Completed, None),
		(_, 0) => (
			FinishStatus::RetrievalOnly,
			Some("The local model is unavailable"),
		),
		(Err(ModelError::Unavailable | ModelError::Interrupted), _) => (
			FinishStatus::Error,
			Some("The local model stopped before completing its answer"),
		),
	}
}

fn emit(send: &EventSink, id: &str, kind: WorkerEventKind) {
	send(WorkerEvent {
		id: id.to_owned(),
		kind,
	});
}

fn utf8_end(text: &str, limit: usize) -> usize {
	let mut end = text.len().min(limit);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	end
}

fn valid_key(value: &str) -> bool {
	!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_whitespace)
}

fn valid_token(value: &str, limit: usize) -> bool {
	!value.is_empty()
		&& value.len() <= limit
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Read a single bounded newline-delimited frame without buffering the rest of an overlong line.
pub fn read_frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
	let mut line = Vec::new();
	loop {
		let bytes = reader.fill_buf()?;
		if bytes.is_empty() {
			return if line.is_empty() {
				Ok(None)
			} else {
				Ok(Some(line))
			};
		}
		let end = bytes.iter().position(|byte| *byte == b'\n');
		let count = end.map_or(bytes.len(), |position| position + 1);
		if line.len() + count > limit {
			return Err(io::Error::new(
				io::ErrorKind::InvalidData,
				"Agent frame exceeds its byte limit",
			));
		}
		line.extend_from_slice(&bytes[..count]);
		reader.consume(count);
		if end.is_some() {
			return Ok(Some(line));
		}
	}
}
