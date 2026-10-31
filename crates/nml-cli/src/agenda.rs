//! `nml agenda`: the tasks of the workspace, by date.

use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use nml_workspace::TaskRecord;

/// The civil date of a day counted from 1970-01-01, as `YYYY-MM-DD`.
fn civil(days: i64) -> String {
	// Howard Hinnant's `civil_from_days`.
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	format!("{year:04}-{month:02}-{day:02}")
}

/// Today, in UTC.
fn today() -> i64 {
	let seconds = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_secs() as i64);
	seconds.div_euclid(86_400)
}

/// When a task is due: the earliest of its deadline, scheduled date and date, and which.
fn when(record: &TaskRecord) -> Option<(&str, &'static str)> {
	let task = &record.task;
	[
		(task.deadline.as_deref(), "deadline"),
		(task.scheduled.as_deref(), "scheduled"),
		(task.date.as_deref(), ""),
	]
	.into_iter()
	.filter_map(|(date, kind)| Some((date?, kind)))
	.min_by_key(|(date, _)| *date)
}

/// The groups of an agenda, in order, and the tasks of each.
pub(crate) fn group(
	tasks: &[TaskRecord],
	today: i64,
	all: bool,
	days: Option<i64>,
) -> Vec<(&'static str, Vec<&TaskRecord>)> {
	let (today_date, week, limit) = (
		civil(today),
		civil(today + 7),
		days.map(|days| civil(today + days)),
	);
	let mut groups: Vec<(&'static str, Vec<&TaskRecord>)> = [
		"Overdue",
		"Today",
		"Next 7 days",
		"Later",
		"Undated",
		"Done",
	]
	.into_iter()
	.map(|name| (name, Vec::new()))
	.collect();
	for record in tasks {
		let date = when(record).map(|(date, _)| &date[..10.min(date.len())]);
		if let (Some(limit), Some(date)) = (&limit, date)
			&& date > limit.as_str()
		{
			continue;
		}
		let group = match date {
			_ if record.task.done && !all => continue,
			_ if record.task.done => 5,
			// With a limit, only what is due before it.
			None if limit.is_some() => continue,
			None => 4,
			Some(date) if date < today_date.as_str() => 0,
			Some(date) if date == today_date => 1,
			Some(date) if date <= week.as_str() => 2,
			Some(_) => 3,
		};
		groups[group].1.push(record);
	}
	for (_, records) in &mut groups {
		records.sort_by(|a, b| {
			let key = |record: &TaskRecord| when(record).map(|(date, _)| date.to_string());
			(key(a).is_none(), key(a), &a.unit).cmp(&(key(b).is_none(), key(b), &b.unit))
		});
	}
	groups.retain(|(_, records)| !records.is_empty());
	groups
}

/// Prints the agenda of the workspace at `root`.
pub(crate) fn agenda(root: &Path, all: bool, days: Option<i64>) -> ExitCode {
	let (tasks, diagnostics) = match nml_build::agenda(root) {
		Ok(agenda) => agenda,
		Err(error) => {
			eprintln!("error: {error}");
			return ExitCode::FAILURE;
		}
	};
	// The parse's errors leave out the tasks of the files that have them.
	let errors = diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.severity == nml_diagnostics::DiagnosticSeverity::Error)
		.count();
	if errors > 0 {
		eprintln!("warning: {errors} error(s) in the workspace: `nml build` shows them");
	}
	let mut out = std::io::stdout().lock();
	let bold = |text: &str| match std::io::stdout().is_terminal() {
		true => format!("\x1b[1m{text}\x1b[0m"),
		false => text.to_string(),
	};
	let today = today();
	let groups = group(&tasks, today, all, days);
	if groups.is_empty() {
		let _ = writeln!(out, "Nothing to do.");
	}
	for (name, records) in groups {
		let _ = writeln!(out, "{}", bold(&format!("{name} ({})", records.len())));
		for record in records {
			let task = &record.task;
			let (date, kind) = when(record).unwrap_or(("", ""));
			let state = match (task.heading, task.state.as_str()) {
				(true, state) => state.to_string(),
				(false, "done") => "[x]".into(),
				(false, "partial") => "[-]".into(),
				(false, _) => "[ ]".into(),
			};
			let date = date.replace('T', " ");
			let _ = writeln!(
				out,
				"  {date:<16} {kind:<9} {state:<5} {}  ({})",
				task.title, task.path
			);
		}
		let _ = writeln!(out);
	}
	ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
	use nml_workspace::StoredTask;

	use super::*;

	#[test]
	fn dates() {
		assert_eq!(civil(0), "1970-01-01");
		assert_eq!(civil(20_393), "2025-11-01");
		assert_eq!(civil(19_782), "2024-02-29");
		assert_eq!(civil(-1), "1969-12-31");
	}

	fn task(title: &str, done: bool, deadline: Option<&str>, date: Option<&str>) -> TaskRecord {
		TaskRecord {
			unit: "plan.nml".into(),
			unit_title: "Plan".into(),
			task: StoredTask {
				heading: false,
				state: if done { "done" } else { "todo" }.into(),
				done,
				title: title.into(),
				date: date.map(String::from),
				scheduled: None,
				deadline: deadline.map(String::from),
				path: "plan.nml".into(),
				range: 0..0,
			},
		}
	}

	#[test]
	fn groups() {
		// 2025-11-01.
		let today = 20_393;
		let tasks = [
			task("later", false, Some("2025-12-01"), None),
			task("overdue", false, None, Some("2025-10-01")),
			task("today", false, Some("2025-11-01T09:00"), None),
			task("week", false, Some("2025-11-05"), Some("2025-11-04")),
			task("undated", false, None, None),
			task("done", true, None, None),
		];
		let shown = |all, days| -> Vec<(&str, Vec<&str>)> {
			(group(&tasks, today, all, days).into_iter())
				.map(|(name, records)| {
					(
						name,
						records.iter().map(|r| r.task.title.as_str()).collect(),
					)
				})
				.collect()
		};
		assert_eq!(
			shown(false, None),
			[
				("Overdue", vec!["overdue"]),
				("Today", vec!["today"]),
				("Next 7 days", vec!["week"]),
				("Later", vec!["later"]),
				("Undated", vec!["undated"]),
			]
		);
		assert_eq!(shown(true, None).last().unwrap(), &("Done", vec!["done"]));
		// Within two days: what is due by then, overdue included.
		assert_eq!(
			shown(false, Some(2)),
			[("Overdue", vec!["overdue"]), ("Today", vec!["today"])]
		);
	}
}
