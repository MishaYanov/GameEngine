use schemars::{JsonSchema, schema::RootSchema, schema_for};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{error::Error, fmt};

const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;
const MAX_QUERY_CHARS: usize = 512;
const MAX_QUERY_RESULTS: u8 = 16;
const DEFAULT_QUERY_RESULTS: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolId {
	EditorInspect,
	AssetsQuery,
	KnowledgeSearch,
}

impl ToolId {
	pub fn name(self) -> &'static str {
		RegisteredTool::by_id(self).name
	}
}

/// Project identity captured by the trusted host before dispatching tool calls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolScope {
	project_id: String,
	workspace_generation: u64,
	revision: u64,
}

impl ToolScope {
	pub fn new(project_id: String, workspace_generation: u64, revision: u64) -> Self {
		Self {
			project_id,
			workspace_generation,
			revision,
		}
	}

	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	pub fn workspace_generation(&self) -> u64 {
		self.workspace_generation
	}

	pub fn revision(&self) -> u64 {
		self.revision
	}
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
	/// An asset name, identifier, or authoring question.
	#[schemars(length(min = 1, max = "MAX_QUERY_CHARS"))]
	query: String,
	/// Maximum number of results.
	#[serde(default = "default_query_results")]
	#[schemars(range(min = 1, max = "MAX_QUERY_RESULTS"))]
	limit: u8,
}

impl SearchArgs {
	pub fn query(&self) -> &str {
		&self.query
	}

	pub fn limit(&self) -> usize {
		self.limit.into()
	}

	fn validate(&self) -> Result<(), ToolError> {
		if self.query.trim().is_empty()
			|| self.query.chars().count() > MAX_QUERY_CHARS
			|| !(1..=MAX_QUERY_RESULTS).contains(&self.limit)
		{
			return Err(ToolError::InvalidArguments);
		}
		Ok(())
	}
}

fn default_query_results() -> u8 {
	DEFAULT_QUERY_RESULTS
}

/// Live adapter contract. Implementations revalidate scope during each read.
pub trait ToolService {
	fn supports(&self, scope: &ToolScope, tool: ToolId) -> bool;
	fn inspect(&self, scope: &ToolScope) -> Result<Value, ToolError>;
	fn query_assets(&self, scope: &ToolScope, args: &SearchArgs) -> Result<Value, ToolError>;
	fn search_knowledge(&self, scope: &ToolScope, args: &SearchArgs) -> Result<Value, ToolError>;
}

#[derive(Debug, Serialize)]
pub struct ToolDescriptor {
	pub name: &'static str,
	pub description: &'static str,
	pub read_only: bool,
	pub input_schema: RootSchema,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ToolError {
	Denied,
	Unavailable(ToolId),
	InvalidArguments,
	StaleScope,
	ResultTooLarge,
	Service(String),
}

impl fmt::Display for ToolError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Denied => formatter.write_str("Tool is outside this agent profile"),
			Self::Unavailable(tool) => write!(
				formatter,
				"{} is unavailable in this authoring session",
				tool.name()
			),
			Self::InvalidArguments => {
				formatter.write_str("Tool arguments do not match the scoped schema")
			}
			Self::StaleScope => formatter.write_str("Authoring scope is stale"),
			Self::ResultTooLarge => formatter.write_str("Tool result exceeds its byte budget"),
			Self::Service(message) => formatter.write_str(message),
		}
	}
}

impl Error for ToolError {}

type Dispatch = fn(&dyn ToolService, &ToolScope, &[u8]) -> Result<Value, ToolError>;

pub(crate) struct RegisteredTool {
	id: ToolId,
	name: &'static str,
	description: &'static str,
	input_schema: fn() -> RootSchema,
	dispatch: Dispatch,
}

impl RegisteredTool {
	const fn new<T: ReadTool>() -> Self {
		Self {
			id: T::ID,
			name: T::NAME,
			description: T::DESCRIPTION,
			input_schema: || schema_for!(T),
			dispatch: |service, scope, json| {
				let input =
					serde_json::from_slice::<T>(json).map_err(|_| ToolError::InvalidArguments)?;
				input.invoke(service, scope)
			},
		}
	}

	pub(crate) fn find(name: &str) -> Option<&'static Self> {
		REGISTRY.iter().find(|tool| tool.name == name)
	}

	pub(crate) fn by_id(id: ToolId) -> &'static Self {
		match id {
			ToolId::EditorInspect => &REGISTRY[0],
			ToolId::AssetsQuery => &REGISTRY[1],
			ToolId::KnowledgeSearch => &REGISTRY[2],
		}
	}

	pub(crate) fn id(&self) -> ToolId {
		self.id
	}

	pub(crate) fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: self.name,
			description: self.description,
			read_only: true,
			input_schema: (self.input_schema)(),
		}
	}

	pub(crate) fn invoke(
		&self,
		service: &dyn ToolService,
		scope: &ToolScope,
		arguments_json: &[u8],
	) -> Result<Value, ToolError> {
		if arguments_json.len() > MAX_TOOL_ARGUMENT_BYTES
			|| arguments_json
				.iter()
				.copied()
				.find(|byte| !byte.is_ascii_whitespace())
				!= Some(b'{')
		{
			return Err(ToolError::InvalidArguments);
		}
		let result = (self.dispatch)(service, scope, arguments_json)?;
		let bytes =
			serde_json::to_vec(&result).map_err(|error| ToolError::Service(error.to_string()))?;
		if bytes.len() > MAX_TOOL_RESULT_BYTES {
			return Err(ToolError::ResultTooLarge);
		}
		Ok(result)
	}
}

const REGISTRY: [RegisteredTool; 3] = [
	RegisteredTool::new::<InspectArgs>(),
	RegisteredTool::new::<AssetQueryArgs>(),
	RegisteredTool::new::<KnowledgeSearchArgs>(),
];

trait ReadTool: DeserializeOwned + JsonSchema {
	const ID: ToolId;
	const NAME: &'static str;
	const DESCRIPTION: &'static str;

	fn invoke(self, service: &dyn ToolService, scope: &ToolScope) -> Result<Value, ToolError>;
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InspectArgs {}

impl ReadTool for InspectArgs {
	const ID: ToolId = ToolId::EditorInspect;
	const NAME: &'static str = "editor.inspect";
	const DESCRIPTION: &'static str =
		"Inspect the captured object and its supported components in the editor.";

	fn invoke(self, service: &dyn ToolService, scope: &ToolScope) -> Result<Value, ToolError> {
		service.inspect(scope)
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
struct AssetQueryArgs(SearchArgs);

impl ReadTool for AssetQueryArgs {
	const ID: ToolId = ToolId::AssetsQuery;
	const NAME: &'static str = "assets.query";
	const DESCRIPTION: &'static str = "Look up known asset metadata within the active project.";

	fn invoke(self, service: &dyn ToolService, scope: &ToolScope) -> Result<Value, ToolError> {
		self.0.validate()?;
		service.query_assets(scope, &self.0)
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
struct KnowledgeSearchArgs(SearchArgs);

impl ReadTool for KnowledgeSearchArgs {
	const ID: ToolId = ToolId::KnowledgeSearch;
	const NAME: &'static str = "knowledge.search";
	const DESCRIPTION: &'static str =
		"Search approved engine guidance and active-project reference material.";

	fn invoke(self, service: &dyn ToolService, scope: &ToolScope) -> Result<Value, ToolError> {
		self.0.validate()?;
		service.search_knowledge(scope, &self.0)
	}
}
