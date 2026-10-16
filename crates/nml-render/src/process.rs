//! Running a program with a time limit.

use std::io;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// A program that ended, or ran out of time.
pub(crate) struct Finished {
	/// How it exited, or `None` if it ran out of time and was killed.
	pub status: Option<ExitStatus>,
	pub out: Vec<u8>,
	pub err: Vec<u8>,
	/// How long it ran.
	pub duration: Duration,
}

/// Runs a program with `input` on its standard input, and waits for it, at most `limit`.
/// An error means it could not be started.
pub(crate) fn wait(command: &mut Command, input: &[u8], limit: Duration) -> io::Result<Finished> {
	let start = Instant::now();
	let mut child = command
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()?;
	let mut stdin = child.stdin.take().unwrap();
	let input = input.to_vec();
	// Written and read on threads of their own, so that full pipes cannot block the child.
	let writer = std::thread::spawn(move || {
		let _ = stdin.write_all(&input);
	});
	let mut stdout = child.stdout.take().unwrap();
	let reader = std::thread::spawn(move || {
		let mut out = Vec::new();
		let _ = stdout.read_to_end(&mut out);
		out
	});
	let mut stderr = child.stderr.take().unwrap();
	let errors = std::thread::spawn(move || {
		let mut out = Vec::new();
		let _ = stderr.read_to_end(&mut out);
		out
	});
	let status = loop {
		match child.try_wait()? {
			Some(status) => break Some(status),
			None if start.elapsed() > limit => {
				let _ = child.kill();
				let _ = child.wait();
				break None;
			}
			None => std::thread::sleep(Duration::from_millis(5)),
		}
	};
	let duration = start.elapsed();
	// A child of the program may keep the pipes open after it was killed: they are not
	// waited for then.
	let (out, err) = match status {
		Some(_) => {
			let _ = writer.join();
			(
				reader.join().unwrap_or_default(),
				errors.join().unwrap_or_default(),
			)
		}
		None => (Vec::new(), Vec::new()),
	};
	Ok(Finished {
		status,
		out,
		err,
		duration,
	})
}
