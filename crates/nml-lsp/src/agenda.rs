//! The agenda: the tasks of the workspace for editors (`nml/agenda`), and toggling a task
//! (`nml.toggleTask`).

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;

use lsp_types::TextEdit;
use lsp_types::Uri;
use lsp_types::WorkspaceEdit;
use nml_document::ElementKind;
use nml_source::Token;
use nml_source::Url;
use serde::Serialize;

use crate::Result;
use crate::Server;
use crate::convert;

/// The request of the workspace's tasks.
pub(crate) const AGENDA: &str = "nml/agenda";

/// Toggles the task on a line: `[uri, line]`.
pub(crate) const TOGGLE_TASK_COMMAND: &str = "nml.toggleTask";

/// A task, as editors see it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgendaTask {
	pub uri: String,
	pub range: lsp_types::Range,
	/// The path of its unit, and its title.
	pub unit: String,
	pub unit_title: String,
	pub heading: bool,
	pub state: String,
	pub done: bool,
	pub title: String,
	pub date: Option<String>,
	pub scheduled: Option<String>,
	pub deadline: Option<String>,
}

impl Server {
	/// The tasks of the workspace: from the index, with the open documents as they are being
	/// edited. A task of an included file is listed once.
	pub(crate) fn agenda(&self) -> Result<Vec<AgendaTask>> {
		// By unit: its title and tasks.
		let mut units: BTreeMap<String, (String, Vec<nml_workspace::StoredTask>)> = BTreeMap::new();
		if let Some(index) = self.index() {
			for record in index.tasks()? {
				let task = record.task;
				let unit = units
					.entry(record.unit)
					.or_insert_with(|| (record.unit_title, Vec::new()));
				unit.1.push(task);
			}
		}
		for analysis in self.analyses.values() {
			if self.root.is_none() || analysis.context.is_some() {
				continue;
			}
			let title = (units.get(&analysis.path))
				.map_or_else(|| analysis.key.clone(), |(title, _)| title.clone());
			let tasks = (analysis.document.tasks().into_iter())
				.filter_map(|task| {
					let root = task.token.root();
					let path = self.workspace_path(root.source().location())?;
					Some(nml_workspace::StoredTask {
						heading: task.heading,
						state: task.state,
						done: task.done,
						title: task.title,
						date: task.date,
						scheduled: task.scheduled,
						deadline: task.deadline,
						path,
						range: root.range(),
					})
				})
				.collect();
			units.insert(analysis.path.clone(), (title, tasks));
		}
		let mut seen = std::collections::HashSet::new();
		let mut out = Vec::new();
		for (unit, (unit_title, tasks)) in units {
			for task in tasks {
				if !seen.insert((task.path.clone(), task.range.start)) {
					continue;
				}
				// Where it is now: the open document, or the file on disk.
				let Some(location) = self.location(&task.path, task.range.clone()) else {
					continue;
				};
				out.push(AgendaTask {
					uri: location.uri.as_str().to_string(),
					range: location.range,
					unit: unit.clone(),
					unit_title: unit_title.clone(),
					heading: task.heading,
					state: task.state,
					done: task.done,
					title: task.title,
					date: task.date,
					scheduled: task.scheduled,
					deadline: task.deadline,
				});
			}
		}
		Ok(out)
	}

	/// The edit that toggles the task on `line`: a checkbox becomes done or not; a heading's
	/// state becomes the first done state, or the first one to do if it was done.
	fn toggle_edit(&self, url: &Url, line: u32) -> Result<(Token, String)> {
		let analysis = (self.analyses.get(url)).ok_or_else(|| format!("`{url}` is not open"))?;
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut checkboxes = Vec::new();
		crate::actions::checkboxes(&analysis.tree, language, url, &mut checkboxes);
		if let Some(checkbox) = checkboxes
			.into_iter()
			.find(|token| convert::range(token).start.line == line)
		{
			let content = checkbox.content();
			let blank = content.len() - content.trim_start().len();
			let token = Token::new(
				checkbox.source().clone(),
				checkbox.start() + blank..checkbox.end(),
			);
			return Ok((token, crate::actions::toggled(content).into()));
		}
		let states = self
			.config
			.as_ref()
			.map(|config| config.agenda.clone())
			.unwrap_or_default();
		let document = &analysis.document;
		for (_, element) in document.descendants(document.root()) {
			let ElementKind::TaskState { done, .. } = element.kind else {
				continue;
			};
			let token = element.token.root();
			if token.source().location() != url || convert::range(&token).start.line != line {
				continue;
			}
			let next = match done {
				true => states.todo.first(),
				false => states.done.first(),
			};
			let next = next.ok_or("the configuration has no state to toggle to")?;
			return Ok((token, next.clone()));
		}
		Err(format!("no task on line {}", line + 1).into())
	}

	/// `nml.toggleTask`: sends the edit that toggles the task on a line.
	pub(crate) fn toggle_task(&mut self, arguments: &[serde_json::Value]) -> Result<()> {
		let (Some(uri), Some(line)) = (
			arguments.first().and_then(serde_json::Value::as_str),
			arguments.get(1).and_then(serde_json::Value::as_u64),
		) else {
			return Err(format!("`{TOGGLE_TASK_COMMAND}` takes a document URI and a line").into());
		};
		let url = Url::parse(uri)?;
		let (token, text) = self.toggle_edit(&url, line as u32)?;
		#[allow(clippy::mutable_key_type)] // See `actions.rs`.
		let changes = HashMap::from([(
			Uri::from_str(uri)?,
			vec![TextEdit::new(convert::range(&token), text)],
		)]);
		let params = lsp_types::ApplyWorkspaceEditParams {
			label: None,
			edit: WorkspaceEdit {
				changes: Some(changes),
				..WorkspaceEdit::default()
			},
		};
		self.send_request("workspace/applyEdit", serde_json::to_value(params)?);
		Ok(())
	}
}
