//! Application-service adapter for the existing typed, read-only Agent tools.

use crate::editor::{
	agent_context::{AgentContextRequest, InspectorAddress},
	EditorWorkspace,
};
use authoring_agent::{SearchArgs, ToolError, ToolId, ToolScope, ToolService};
use runtime::assets::project_assets::load_catalog;
use serde_json::{json, Value};
use std::{
	path::Path,
	sync::{Arc, Mutex},
};

pub(super) type WorkspaceSlot = Arc<Mutex<Option<EditorWorkspace>>>;

pub(super) struct LiveTools<'a> {
	shared: &'a WorkspaceSlot,
	generation: u64,
	authorizes: &'a dyn Fn() -> bool,
}

impl<'a> LiveTools<'a> {
	pub(super) fn new(
		shared: &'a WorkspaceSlot,
		generation: u64,
		authorizes: &'a dyn Fn() -> bool,
	) -> Self {
		Self {
			shared,
			generation,
			authorizes,
		}
	}

	pub(super) fn scope(&self) -> Option<ToolScope> {
		self.with_workspace(|workspace| Ok(self.scope_of(workspace)))
			.ok()
	}

	fn scope_of(&self, workspace: &EditorWorkspace) -> ToolScope {
		ToolScope::new(
			format!("workspace-{}", self.generation),
			self.generation,
			workspace.backend.session().revision(),
		)
	}

	fn with_workspace<T>(
		&self,
		read: impl FnOnce(&EditorWorkspace) -> Result<T, ToolError>,
	) -> Result<T, ToolError> {
		if !(self.authorizes)() {
			return Err(ToolError::StaleScope);
		}
		let slot = self.shared.lock().map_err(|_| ToolError::StaleScope)?;
		let workspace = slot
			.as_ref()
			.filter(|workspace| workspace.generation == self.generation)
			.ok_or(ToolError::StaleScope)?;
		read(workspace)
	}

	fn read_scoped<T>(
		&self,
		scope: &ToolScope,
		read: impl FnOnce(&EditorWorkspace) -> Result<T, ToolError>,
	) -> Result<T, ToolError> {
		self.with_workspace(|workspace| {
			if self.scope_of(workspace) != *scope {
				return Err(ToolError::StaleScope);
			}
			let session = workspace.backend.session();
			if session.is_playing() || session.has_transaction() {
				return Err(ToolError::Service(
					"Stop Play and finish the current edit before inspecting the workspace".into(),
				));
			}
			read(workspace)
		})
	}
}

impl ToolService for LiveTools<'_> {
	fn supports(&self, scope: &ToolScope, tool: ToolId) -> bool {
		matches!(tool, ToolId::EditorInspect | ToolId::AssetsQuery)
			&& self.scope().as_ref() == Some(scope)
	}

	fn inspect(&self, scope: &ToolScope) -> Result<Value, ToolError> {
		let (scene, captured) = self.read_scoped(scope, |workspace| {
			let session = workspace.backend.session();
			let scene = session.active_scene_id().clone();
			let captured = session
				.selection()
				.first()
				.map(|key| {
					let failure =
						|| ToolError::Service("The selected object is unavailable".into());
					let linked = workspace
						.linked_prop_address(key.as_str(), workspace.generation, scope.revision())
						.map_err(|_| failure())?;
					let address = match linked {
						Some(linked) => InspectorAddress::Linked {
							scene: linked.scene_id,
							root: linked.root_key,
							address: linked.address,
						},
						None => InspectorAddress::Scene {
							scene: scene.clone(),
							object: key.clone(),
						},
					};
					workspace
						.capture_agent_context(AgentContextRequest {
							workspace_generation: workspace.generation,
							revision: scope.revision(),
							content_revision: session.scene_revision(),
							address,
							component_id: None,
						})
						.map_err(|_| failure())
				})
				.transpose()?;
			Ok((scene, captured))
		})?;
		let selection = captured
			.map(|selection| selection.resolve())
			.transpose()
			.map_err(|_| ToolError::Service("The selected object is unavailable".into()))?;
		Ok(json!({
			"workspace_generation": scope.workspace_generation(),
			"revision": scope.revision(),
			"scene": scene,
			"selection": selection
		}))
	}

	fn query_assets(&self, scope: &ToolScope, args: &SearchArgs) -> Result<Value, ToolError> {
		let root = self.read_scoped(scope, |workspace| Ok(workspace.asset_root.clone()))?;
		verify_asset_root(&root)?;
		let catalog = load_catalog(&root)
			.map_err(|_| ToolError::Service("The project asset catalog is unavailable".into()))?;
		verify_asset_root(&root)?;
		let query = args.query().trim().to_lowercase();
		let assets: Vec<_> = catalog
			.assets
			.iter()
			.filter(|asset| {
				asset.id.as_str() == query || asset.path.to_lowercase().contains(&query)
			})
			.take(args.limit())
			.map(|asset| json!({"id":asset.id,"path":asset.path,"kind":asset.kind}))
			.collect();
		Ok(
			json!({"workspace_generation":scope.workspace_generation(),"revision":scope.revision(),"assets":assets}),
		)
	}

	fn search_knowledge(&self, _: &ToolScope, _: &SearchArgs) -> Result<Value, ToolError> {
		Err(ToolError::Denied)
	}
}

fn verify_asset_root(root: &Path) -> Result<(), ToolError> {
	if !root.canonicalize().is_ok_and(|canonical| canonical == root) {
		return Err(ToolError::Service(
			"The project asset root changed. Reopen the project before sharing asset metadata."
				.into(),
		));
	}
	Ok(())
}
