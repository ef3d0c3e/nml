use mlua::Lua;
use mlua::Table;

use crate::api;
use crate::files::Files;

/// The environment variable of reproducible builds: the time to use instead of the clock.
const SOURCE_DATE_EPOCH: &str = "SOURCE_DATE_EPOCH";

/// Reads an environment variable and records it, with the hash of its value.
fn read_env(files: &mut Files, name: &str) -> Option<String> {
	let value = files.resolver.env(name);
	if !files.env.iter().any(|(read, _)| read == name) {
		let hash = nml_lang::env_hash(value.as_deref());
		files.env.push((name.to_string(), hash));
	}
	value
}

/// The value of an environment variable of the build, or `nil` if it is not set.
///
/// The document is built again when the value changes.
#[api]
fn env(lua: &Lua, name: String) -> mlua::Result<Option<String>> {
	let mut files = Files::of(lua, "`nml.env`")?;
	Ok(read_env(&mut files, &name))
}

/// When the build started, in seconds since the Unix epoch: the same for every document.
///
/// If `SOURCE_DATE_EPOCH` is set, its value instead, for reproducible builds; the document
/// is built again when it changes. Otherwise, the document is built again on every build.
#[api]
fn time(lua: &Lua) -> mlua::Result<i64> {
	let mut files = Files::of(lua, "`nml.time`")?;
	match read_env(&mut files, SOURCE_DATE_EPOCH) {
		Some(value) => value.trim().parse().map_err(|_| {
			mlua::Error::runtime(format!(
				"`{SOURCE_DATE_EPOCH}` is not a number of seconds: `{value}`"
			))
		}),
		None => {
			files.clock = true;
			Ok(files.resolver.time())
		}
	}
}

/// The calendar fields of a time in UTC, as `os.date("!*t", time)` gives them: `year`,
/// `month`, `day`, `hour`, `min`, `sec`, `wday` (1 is Sunday), `yday` and `isdst`.
///
/// `time` is in seconds since the Unix epoch; by default, `nml.time()`.
#[api]
fn date(lua: &Lua, time: Option<i64>) -> mlua::Result<Table> {
	let time = match time {
		Some(time) => time,
		None => self::time(lua)?,
	};
	let days = time.div_euclid(86_400);
	let seconds = time.rem_euclid(86_400);
	let (year, month, day) = civil(days);
	let first = days_from_civil(year, 1, 1);
	let table = lua.create_table()?;
	table.set("year", year)?;
	table.set("month", month)?;
	table.set("day", day)?;
	table.set("hour", seconds / 3600)?;
	table.set("min", seconds % 3600 / 60)?;
	table.set("sec", seconds % 60)?;
	// 1970-01-01 was a Thursday.
	table.set("wday", (days + 4).rem_euclid(7) + 1)?;
	table.set("yday", days - first + 1)?;
	table.set("isdst", false)?;
	Ok(table)
}

/// The date of a day counted from 1970-01-01, in the proleptic Gregorian calendar (Howard
/// Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	(year, month, day)
}

/// The day counted from 1970-01-01 of a date (`days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let yoe = year.rem_euclid(400);
	let mp = if month > 2 { month - 3 } else { month + 9 };
	let doy = (153 * mp + 2) / 5 + day - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;
	use nml_lang::MemoryResolver;

	use super::*;
	use crate::tests::compile_with_resolver;
	use crate::tests::html;

	fn resolver(env: &[(&str, &str)]) -> MemoryResolver {
		MemoryResolver {
			env: env
				.iter()
				.map(|(name, value)| (name.to_string(), value.to_string()))
				.collect(),
			time: 1_700_000_000,
			..MemoryResolver::default()
		}
	}

	#[test]
	fn environment() {
		let compiled = compile_with_resolver(
			":lua`nml.env('SITE')` :lua`tostring(nml.env('NONE'))` :lua`nml.env('SITE')`\n",
			resolver(&[("SITE", "prod")]),
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>prod nil prod</p>"));
		// Each variable once, with the hash of its value; none for an unset one.
		assert_eq!(
			compiled.unit.dependencies,
			[
				Dependency::Env {
					name: "SITE".into(),
					hash: nml_lang::env_hash(Some("prod")),
				},
				Dependency::Env {
					name: "NONE".into(),
					hash: String::new(),
				},
			]
		);
		assert_ne!(nml_lang::env_hash(Some("")), "");
	}

	#[test]
	fn clock() {
		let compiled = compile_with_resolver(":lua`nml.time()` :lua`nml.time()`\n", resolver(&[]));
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>1700000000 1700000000</p>"));
		assert_eq!(
			compiled.unit.dependencies,
			[
				Dependency::Env {
					name: SOURCE_DATE_EPOCH.into(),
					hash: String::new(),
				},
				Dependency::Clock,
			]
		);

		// With `SOURCE_DATE_EPOCH`, the clock is not read.
		let compiled = compile_with_resolver(
			":lua`nml.time()`\n",
			resolver(&[(SOURCE_DATE_EPOCH, "86400")]),
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>86400</p>"));
		assert_eq!(
			compiled.unit.dependencies,
			[Dependency::Env {
				name: SOURCE_DATE_EPOCH.into(),
				hash: nml_lang::env_hash(Some("86400")),
			}]
		);

		let compiled = compile_with_resolver(
			":lua`nml.time()`\n",
			resolver(&[(SOURCE_DATE_EPOCH, "yesterday")]),
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn dates() {
		let compiled = compile_with_resolver(
			":lua\n```\nfunction show(d)\n  return string.format('%04d-%02d-%02d %02d:%02d:%02d w%d y%d %s', d.year, d.month, d.day, d.hour, d.min, d.sec, d.wday, d.yday, tostring(d.isdst))\nend\n```\n\n:lua`show(nml.date(0))`\n\n:lua`show(nml.date(951782400))`\n\n:lua`show(nml.date(-1))`\n\n:lua`show(nml.date())`\n",
			resolver(&[]),
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		assert!(html.contains("1970-01-01 00:00:00 w5 y1 false"), "{html}");
		// A leap day, in a year divisible by 400.
		assert!(html.contains("2000-02-29 00:00:00 w3 y60 false"), "{html}");
		assert!(html.contains("1969-12-31 23:59:59 w4 y365 false"), "{html}");
		// 2023-11-14 22:13:20 UTC, a Tuesday.
		assert!(html.contains("2023-11-14 22:13:20 w3 y318 false"), "{html}");
		// Only the date of `nml.time()` reads the clock.
		assert!(compiled.unit.dependencies.contains(&Dependency::Clock));
	}

	#[test]
	fn calendar_round_trip() {
		for days in -800_000..800_000 {
			let (year, month, day) = civil(days);
			assert!((1..=12).contains(&month) && (1..=31).contains(&day));
			assert_eq!(days_from_civil(year, month, day), days);
		}
	}
}
