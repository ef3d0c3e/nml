use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Alignment;
use nml_document::Attributes;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::SymbolKind;
use nml_grammar::Atom;
use nml_grammar::Capture;
use nml_grammar::Node;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Options;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_grammar::Value;

use crate::Language;
use crate::Lower;
use crate::language::plain_text;

/// Tables: rows of cells between pipes, `| a | b |`. A separator row, `|:--|--:|`, makes the
/// rows above it header rows and sets the alignment of the columns. A cell can start with
/// options right after its pipe: `|[hspan=2] wide |`.
///
/// `:table [#ref] [Caption]` before a table numbers it, gives it a caption and makes it
/// referenceable.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let span = |name: &str, doc: &str| {
		OptionSpec::new(name, OptionKind::Integer)
			.with_default(OptionValue::Integer(1))
			.with_doc(doc)
	};
	let schema = Arc::new(OptionSchema::new([
		span("hspan", "How many columns the cell spans."),
		span("vspan", "How many rows the cell spans."),
	]));
	let cell = || {
		vec![
			Atom::capture("pipe", Atom::literal("|")).with_class("operator"),
			Atom::optional(vec![Atom::capture(
				"options",
				Atom::Options(schema.clone()),
			)]),
			Atom::capture("cell", Atom::InlineContent),
		]
	};
	// A row: its first cell, the others, and the end of its line.
	let row = || {
		let mut atoms = vec![Atom::LineStart];
		atoms.extend(cell());
		atoms.push(Atom::repeat(cell(), 0, None));
		atoms.push(Atom::capture("row_end", Atom::LineEnd));
		atoms
	};
	let mut atoms = row();
	atoms.push(Atom::repeat(row(), 0, None));
	let rows = Rule::new("table_rows", atoms)
		.with_doc(
			"**Table**: rows of cells between pipes.\n\n\
			```nml\n| Name | Price |\n|:-----|------:|\n| Tea  |     3 |\n```\n\n\
			A separator row makes the rows above it headers, and aligns the columns with `:`. \
			A cell's options come right after its pipe: `|[hspan=2] wide |`, `vspan` for rows.",
		)
		.with_snippet("| ${1:Header} | ${2:Header} |\n|---|---|\n| ${3} | ${0} |");
	language.add(rows, lower_table)?;

	let command = Rule::new(
		"table",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":table")).with_class("keyword"),
			Atom::optional(vec![
				Atom::literal(" #"),
				Atom::capture("ref", Atom::Word).with_class("variable"),
			]),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("caption", Atom::InlineContent),
			]),
			Atom::LineEnd,
			Atom::capture("body", Atom::Block),
		],
	)
	.with_doc(
		"**Numbered table**: the table after it, with a caption.\n\n\
		```nml\n:table #prices Prices\n| Tea | 3 |\n```\n\n\
		`#name` makes it referenceable: `[[prices]]` shows *Table 1*.",
	)
	.with_snippet(":table #${1:name} ${2:Caption}\n| ${3} | ${0} |");
	language.add(command, lower_command)?;
	Ok(())
}

/// A cell as written: its options and content.
struct Cell<'a> {
	options: Option<&'a Options>,
	capture: &'a Capture,
}

/// What a `:table` gives the table it applies to.
struct Pending {
	number: u32,
	id: Option<String>,
	caption: Option<Vec<Node>>,
	/// The table's element, once it is written.
	element: Option<ElementId>,
}

/// The number of the last numbered table of a unit, and the `:table` being lowered.
#[derive(Default)]
struct Tables {
	count: u32,
	pending: Option<Pending>,
}

fn lower_command(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	// The block must be a table itself, not hold one.
	let rules = lower.language().rules();
	let is_table = match m.capture("body").map(|capture| &capture.value) {
		Some(Value::Nodes(nodes)) => nodes.iter().any(|node| {
			matches!(node, Node::Match(table)
				if rules.get(table.rule).is_some_and(|rule| rule.name == "table_rows"))
		}),
		_ => false,
	};
	if !is_table {
		let command = m.capture("command").unwrap().token.clone();
		let message = DiagnosticMessage::new(command);
		let diagnostic = Diagnostic::new(DiagnosticKind::NotATable, message);
		lower.diagnostics.push(diagnostic);
		lower.capture(m, "body");
		return;
	}
	let tables = lower.extension(Tables::default);
	tables.count += 1;
	let number = tables.count;
	let reference = m.capture("ref").map(|capture| capture.token.clone());
	let caption = match m.capture("caption").map(|capture| &capture.value) {
		Some(Value::Nodes(nodes)) => Some(nodes.clone()),
		_ => None,
	};
	let title = caption.as_deref().map(plain_text).unwrap_or_default();
	lower.extension(Tables::default).pending = Some(Pending {
		number,
		id: reference.as_ref().map(|r| r.content().to_string()),
		caption,
		element: None,
	});
	lower.capture(m, "body");
	let pending = lower.extension(Tables::default).pending.take();
	// An incomplete table is not lowered, and is already an error.
	let Some(element) = pending.and_then(|pending| pending.element) else {
		return;
	};
	if let Some(reference) = reference {
		let export = Export::new(reference.content(), SymbolKind::Table, Some(element))
			.with("title", title)
			.with("number", number.to_string());
		lower.export(export, reference);
	}
}

fn lower_table(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	// The rows, as cells.
	let mut rows: Vec<Vec<Cell>> = vec![Vec::new()];
	let mut options = None;
	for capture in &m.captures {
		match &*capture.name {
			"options" => {
				if let Value::Options(value) = &capture.value {
					options = Some(value);
				}
			}
			"cell" => rows.last_mut().unwrap().push(Cell {
				options: options.take(),
				capture,
			}),
			"row_end" => rows.push(Vec::new()),
			_ => {}
		}
	}
	rows.pop();
	for row in &mut rows {
		// The cell after a row's last pipe.
		if row.len() > 1
			&& row.last().is_some_and(|cell| {
				cell.options.is_none() && cell.capture.token.content().trim().is_empty()
			}) {
			row.pop();
		}
	}

	let separator = rows.iter().position(|row| row.iter().all(is_separator));
	let (alignments, header_rows) = match separator {
		Some(index) => {
			let alignments = rows[index].iter().map(alignment).collect();
			rows.remove(index);
			(alignments, index)
		}
		None => (Vec::new(), 0),
	};

	let pending = lower
		.extension_mut::<Tables>()
		.and_then(|tables| tables.pending.as_mut())
		.filter(|pending| pending.element.is_none())
		.map(|pending| (pending.number, pending.id.clone(), pending.caption.take()));
	let (number, id, caption) = match pending {
		Some((number, id, caption)) => (Some(number), id, caption),
		None => (None, None, None),
	};
	let kind = ElementKind::Table {
		alignments,
		header_rows,
		number,
	};
	let attributes = Attributes {
		id,
		classes: Vec::new(),
	};
	lower.builder.open_with(kind, m.token.clone(), attributes);
	if let Some(caption) = caption {
		lower.builder.open(ElementKind::Caption, m.token.clone());
		lower.nodes(&caption);
		lower.builder.close();
	}
	for row in rows {
		let first = &row[0].capture.token;
		let last = &row[row.len() - 1].capture.token;
		let token = nml_source::Token::new(first.source().clone(), first.start()..last.end());
		lower.builder.open(ElementKind::TableRow, token);
		for cell in row {
			let span = |name| match cell.options.and_then(|options| options.get(name)) {
				Some(OptionValue::Integer(span)) => *span,
				_ => 1,
			};
			let (hspan, vspan) = (span("hspan"), span("vspan"));
			for (name, value) in [("hspan", hspan), ("vspan", vspan)] {
				if value < 1 {
					let entry = cell.options.and_then(|options| {
						options.entries.iter().find(|e| e.key.content() == name)
					});
					let location = entry
						.and_then(|entry| entry.value_token.clone())
						.unwrap_or_else(|| cell.capture.token.clone());
					let kind = DiagnosticKind::InvalidOptionValue {
						name: name.into(),
						expected: "Positive".into(),
					};
					lower
						.diagnostics
						.push(Diagnostic::new(kind, DiagnosticMessage::new(location)));
				}
			}
			let clamp = |span: i64| u32::try_from(span.max(1)).unwrap_or(u32::MAX);
			let kind = ElementKind::TableCell {
				hspan: clamp(hspan),
				vspan: clamp(vspan),
			};
			lower.builder.open(kind, cell.capture.token.clone());
			if let Value::Nodes(nodes) = &cell.capture.value {
				lower.nodes(nodes);
			}
			lower.builder.close();
		}
		lower.builder.close();
	}
	let element = lower.builder.close();
	if let Some(pending) = lower
		.extension_mut::<Tables>()
		.and_then(|tables| tables.pending.as_mut())
		.filter(|_| number.is_some())
	{
		pending.element = Some(element);
	}
}

/// Whether a cell is part of a separator row: dashes, with an optional colon at each end.
fn is_separator(cell: &Cell) -> bool {
	let text = cell.capture.token.content().trim();
	let dashes = text.trim_start_matches(':').trim_end_matches(':');
	cell.options.is_none() && !dashes.is_empty() && dashes.chars().all(|c| c == '-')
}

fn alignment(cell: &Cell) -> Alignment {
	let text = cell.capture.token.content().trim();
	match (text.starts_with(':'), text.ends_with(':')) {
		(true, true) => Alignment::Center,
		(true, false) => Alignment::Left,
		(false, true) => Alignment::Right,
		(false, false) => Alignment::Default,
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Alignment;
	use nml_document::ElementKind;

	use crate::tests::check;
	use crate::tests::compile_text;

	fn table(text: &str) -> ElementKind {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.find(|(_, element)| matches!(element.kind, ElementKind::Table { .. }))
			.unwrap()
			.1
			.kind
			.clone()
	}

	#[test]
	fn tables() {
		let compiled = check(
			"| Plan | Price |\n|:-----|------:|\n| Free | **0** |\n| Pro | 9 |\nafter",
			r#"document[table[table_row[table_cell[" Plan "] table_cell[" Price "]] table_row[table_cell[" Free "] table_cell[" " Bold["0"] " "]] table_row[table_cell[" Pro "] table_cell[" 9 "]]] paragraph["after"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(
			table("| a | b | c | d |\n|---|:--|:-:|--:|\n| 1 | 2 | 3 | 4 |"),
			ElementKind::Table {
				alignments: vec![
					Alignment::Default,
					Alignment::Left,
					Alignment::Center,
					Alignment::Right
				],
				header_rows: 1,
				number: None,
			}
		);
		// Without a separator, no row is a header; without a trailing pipe, the last cell
		// ends with the line.
		check(
			"| a | b\n|c|",
			r#"document[table[table_row[table_cell[" a "] table_cell[" b"]] table_row[table_cell["c"]]]]"#,
		);
		assert_eq!(
			table("|a|\n|b|"),
			ElementKind::Table {
				alignments: vec![],
				header_rows: 0,
				number: None,
			}
		);
		// Empty cells are kept, except after the last pipe.
		check(
			"| a || c |",
			r#"document[table[table_row[table_cell[" a "] table_cell[] table_cell[" c "]]]]"#,
		);
		// Cells hold inline markup; `|` in a code span or escaped does not end a cell.
		check(
			"| `a|b` | \\| | [[x|y]] |",
			r#"document[table[table_row[table_cell[" " code[] " "] table_cell[" | "] table_cell[" " reference["y"] " "]]]]"#,
		);
	}

	#[test]
	fn spans() {
		let compiled = compile_text("|[hspan=2] a |\n|[vspan=3, hspan=1] b | c |");
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let spans: Vec<_> = document
			.descendants(document.root())
			.filter_map(|(_, element)| match element.kind {
				ElementKind::TableCell { hspan, vspan } => Some((hspan, vspan)),
				_ => None,
			})
			.collect();
		assert_eq!(spans, [(2, 1), (1, 3), (1, 1)]);

		let compiled = compile_text("|[hspan=0] a |");
		assert_diagnostics!(compiled.diagnostics, [1007 @ 8..9]);
		let compiled = compile_text("|[span=2] a |");
		assert_diagnostics!(compiled.diagnostics, [1005]);
	}

	#[test]
	fn table_command() {
		let compiled = check(
			":table #prices Prices per **plan**\n| a |\n\n:table\n| b |",
			r#"document[table#prices[caption["Prices per " Bold["plan"]] table_row[table_cell[" a "]]] table[table_row[table_cell[" b "]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let export = &compiled.unit.exports[0];
		assert_eq!(export.name, "prices");
		assert_eq!(export.property("title"), Some("Prices per plan"));
		assert_eq!(export.property("number"), Some("1"));
		assert_eq!(
			table(":table\n| b |"),
			ElementKind::Table {
				alignments: vec![],
				header_rows: 0,
				number: Some(1),
			}
		);

		let compiled = compile_text(":table #t T\nnot a table");
		assert_diagnostics!(compiled.diagnostics, [2019 @ 0..6]);
		// A table inside the block a `:table` applies to is not the table.
		let compiled = compile_text(":table\n- | a |");
		assert_diagnostics!(compiled.diagnostics, [2019]);
	}
}
