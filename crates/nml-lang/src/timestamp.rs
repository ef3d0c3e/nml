//! Timestamps: `<2026-11-20>`, with a weekday and a time, `<2026-11-20 Fri 14:00>`, and
//! `DEADLINE <…>` or `SCHEDULED <…>` for a task's deadline or start.

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::Edit;
use nml_diagnostics::Fix;
use nml_diagnostics::MessageContent;
use nml_document::ElementKind;
use nml_document::TimestampKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_source::Token;

use crate::Language;
use crate::Lower;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The day of the week of a date, from 0 for Monday.
pub fn weekday(year: i64, month: u32, day: u32) -> usize {
	// Sakamoto's method, with Sunday as 0.
	const OFFSETS: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
	let year = if month < 3 { year - 1 } else { year };
	let sunday_first = (year + year.div_euclid(4) - year.div_euclid(100)
		+ year.div_euclid(400)
		+ OFFSETS[month as usize - 1]
		+ day as i64)
		.rem_euclid(7);
	((sunday_first + 6) % 7) as usize
}

/// The weekday of a date written `YYYY-MM-DD`, `Mon` to `Sun`, or `None` if it is no date.
pub fn date_weekday(date: &str) -> Option<&'static str> {
	let mut parts = date.split('-');
	let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
	if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
		return None;
	}
	let (year, month, day): (i64, u32, u32) =
		(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
	if !(1..=12).contains(&month) || !(1..=days_in(year, month)).contains(&day) {
		return None;
	}
	Some(WEEKDAYS[weekday(year, month, day)])
}

/// How many days a month has.
fn days_in(year: i64, month: u32) -> u32 {
	match month {
		2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
		2 => 28,
		4 | 6 | 9 | 11 => 30,
		_ => 31,
	}
}

pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	for (name, opener, kind) in [
		("timestamp", "<", TimestampKind::Date),
		("deadline", "DEADLINE <", TimestampKind::Deadline),
		("scheduled", "SCHEDULED <", TimestampKind::Scheduled),
	] {
		let rule = Rule::new(
			name,
			vec![
				Atom::capture("open", Atom::literal(opener)).with_class("keyword"),
				Atom::capture(
					"year",
					Atom::Number {
						signed: false,
						decimal: false,
					},
				)
				.with_class("number"),
				Atom::literal("-"),
				Atom::capture(
					"month",
					Atom::Number {
						signed: false,
						decimal: false,
					},
				)
				.with_class("number"),
				Atom::literal("-"),
				Atom::capture(
					"day",
					Atom::Number {
						signed: false,
						decimal: false,
					},
				)
				.with_class("number"),
				Atom::capture("rest", Atom::Text).with_class("number"),
				Atom::capture("close", Atom::literal(">")).with_class("keyword"),
			],
		)
		// `a < b` and `<b>` are text.
		.guarded(6)
		.with_doc(
			"**Timestamp**: a date, `<2026-11-20>`, with an optional weekday and time: \
			`<2026-11-20 Fri 14:00>`. `DEADLINE <…>` is the deadline of the task it is in, \
			`SCHEDULED <…>` when to start it; the agenda lists tasks by them.",
		)
		.with_snippet(format!("{opener}${{1:2026-01-01}}>"));
		language.add(rule, move |lower, m| lower_timestamp(lower, m, kind))?;
	}
	Ok(())
}

fn lower_timestamp(lower: &mut Lower<'_, '_>, m: &RuleMatch, kind: TimestampKind) {
	let token = |name| m.capture(name).unwrap().token.clone();
	let invalid = |lower: &mut Lower<'_, '_>| {
		let date = m
			.token
			.content()
			.trim_start_matches(|c| c != '<')
			.to_string();
		let kind = DiagnosticKind::InvalidTimestamp { timestamp: date };
		lower.diagnostics.push(Diagnostic::new(
			kind,
			DiagnosticMessage::new(m.token.clone()),
		));
	};
	let (year, month, day) = (token("year"), token("month"), token("day"));
	let (Ok(year_value), Ok(month_value), Ok(day_value)) = (
		year.content().parse::<i64>(),
		month.content().parse::<u32>(),
		day.content().parse::<u32>(),
	) else {
		return invalid(lower);
	};
	if year.content().len() != 4
		|| month.content().len() != 2
		|| day.content().len() != 2
		|| !(1..=12).contains(&month_value)
		|| !(1..=days_in(year_value, month_value)).contains(&day_value)
	{
		return invalid(lower);
	}
	let actual = WEEKDAYS[weekday(year_value, month_value, day_value)];
	// The rest: a weekday, a time, or both.
	let rest = token("rest");
	let mut time = None;
	for part in rest.content().split_whitespace() {
		if part.len() == 5 && part.as_bytes()[2] == b':' {
			let (hours, minutes) = (part[..2].parse::<u32>(), part[3..].parse::<u32>());
			match (hours, minutes) {
				(Ok(hours), Ok(minutes)) if hours < 24 && minutes < 60 && time.is_none() => {
					time = Some(part.to_string());
				}
				_ => return invalid(lower),
			}
		} else if WEEKDAYS
			.iter()
			.any(|weekday| weekday.eq_ignore_ascii_case(part))
		{
			if !actual.eq_ignore_ascii_case(part) {
				let offset = rest.start() + rest.content().find(part).unwrap();
				let written = Token::new(rest.source().clone(), offset..offset + part.len());
				let kind = DiagnosticKind::WrongWeekday {
					written: part.into(),
					actual: actual.into(),
				};
				let fix = Fix {
					title: MessageContent::Fluent {
						attribute: "fix",
						args: Vec::new(),
					},
					edits: vec![Edit {
						location: written.clone(),
						replacement: actual.into(),
					}],
				};
				let diagnostic =
					Diagnostic::new(kind, DiagnosticMessage::new(written)).with_fix(fix);
				lower.diagnostics.push(diagnostic);
			}
		} else {
			return invalid(lower);
		}
	}
	let kind = ElementKind::Timestamp {
		kind,
		date: format!("{}-{}-{}", year.content(), month.content(), day.content()),
		weekday: actual.into(),
		time,
	};
	lower.builder.leaf(kind, m.token.clone());
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;

	use crate::tests::compile_text;

	fn timestamps(text: &str) -> Vec<String> {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		(document.descendants(document.root()))
			.filter_map(|(_, element)| match &element.kind {
				ElementKind::Timestamp {
					kind,
					date,
					weekday,
					time,
				} => Some(format!(
					"{} {date} {weekday} {}",
					kind.name(),
					time.as_deref().unwrap_or("-")
				)),
				_ => None,
			})
			.collect()
	}

	#[test]
	fn dates() {
		assert_eq!(
			timestamps(
				"Meet <2026-11-20 Fri 14:00>, due DEADLINE <2024-02-29>, from SCHEDULED <2026-01-01 thu>.\n"
			),
			[
				"date 2026-11-20 Fri 14:00",
				"deadline 2024-02-29 Thu -",
				"scheduled 2026-01-01 Thu -"
			]
		);
		// Not timestamps: text.
		assert!(timestamps("a < b, <b>, x <-> y, DEADLINE soon, < 2026-01-01>\n").is_empty());
	}

	#[test]
	fn weekdays() {
		for (date, expected) in [
			((2026, 11, 1), "Sun"),
			((2000, 1, 1), "Sat"),
			((1970, 1, 1), "Thu"),
			((2024, 2, 29), "Thu"),
			((2100, 3, 1), "Mon"),
		] {
			assert_eq!(
				super::WEEKDAYS[super::weekday(date.0, date.1, date.2)],
				expected,
				"{date:?}"
			);
		}
	}

	#[test]
	fn errors() {
		let errors = |text: &str| compile_text(text).diagnostics;
		assert_diagnostics!(errors("<2026-02-30>\n"), [2037 @ 0..12]);
		assert_diagnostics!(errors("<2026-13-01>\n"), [2037]);
		assert_diagnostics!(errors("<2026-1-01>\n"), [2037]);
		assert_diagnostics!(errors("<2026-01-01 25:00>\n"), [2037]);
		assert_diagnostics!(errors("<2026-01-01 soon>\n"), [2037]);
		// A wrong weekday is a warning, with its fix.
		let diagnostics = errors("<2026-11-20 Mon>\n");
		assert_diagnostics!(diagnostics, [2038 @ 12..15]);
		assert_eq!(diagnostics[0].fixes[0].edits[0].replacement, "Fri");
	}
}

#[cfg(test)]
mod task_tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;
	use crate::tests::compile_text;

	#[test]
	fn heading_states() {
		let compiled = check(
			"# TODO Write *it* {#w}\n## DONE\n# TODOS are words\n# Done is not a keyword\n",
			r#"document[h1#w[task_state[] "Write " Italic["it"]] h2[task_state[]] h1["TODOS are words"] h1["Done is not a keyword"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		// The title of the heading, for references, has no state.
		assert_eq!(compiled.unit.exports[0].property("title"), Some("Write it"));
	}

	#[test]
	fn tasks() {
		let compiled = compile_text(concat!(
			"# TODO Release [/]\nSCHEDULED <2026-11-15> DEADLINE <2026-11-20 Fri 18:00>\n\n",
			" * [ ] Changelog <2026-11-12>\n ** [x] Draft it\n * [-] Partly\n * Not a task\n\n",
			"## DONE Tag\n\nText <2026-01-01>.\n",
		));
		assert_diagnostics!(compiled.diagnostics, []);
		let tasks: Vec<_> = (compiled.unit.document.tasks().into_iter())
			.map(|task| {
				format!(
					"{} {} {} {:?} | {} {} {}",
					if task.heading { "heading" } else { "item" },
					task.state,
					task.done,
					task.title,
					task.date.as_deref().unwrap_or("-"),
					task.scheduled.as_deref().unwrap_or("-"),
					task.deadline.as_deref().unwrap_or("-"),
				)
			})
			.collect();
		assert_eq!(
			tasks,
			[
				// The paragraph of timestamps after a heading is its own.
				"heading TODO false \"Release\" | - 2026-11-15 2026-11-20T18:00",
				"item todo false \"Changelog\" | 2026-11-12 - -",
				"item done true \"Draft it\" | - - -",
				"item partial false \"Partly\" | - - -",
				// A paragraph with more than timestamps is not.
				"heading DONE true \"Tag\" | - - -",
			]
		);
		// The heading counts its section's tasks, headings with a state included.
		let document = &compiled.unit.document;
		let progress = (document.descendants(document.root()))
			.find(|(_, element)| matches!(element.kind, nml_document::ElementKind::Progress { .. }))
			.map(|(id, _)| document.progress(id).unwrap());
		assert_eq!(progress, Some((2, 4)));
	}
}
