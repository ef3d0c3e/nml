use std::collections::HashMap;
use std::sync::MutexGuard;

use rusqlite::Connection;

use crate::unit::references::Refname;
use crate::unit::variable::VariableName;

use super::output::CompilerOutput;

pub enum UnitQuery {
	Reference(Refname, String),
	Variable(VariableName, String),
}

/// Stores the extracted data from the compilation unit after calling the compiler on them
///
/// The purpose of this calss is to be storable inside a database
pub struct CompiledUnit {
	/// Output file of this unit
	output_file: String,
	/// Input file of this unit
	input_file: String,
	/// Stores the time in seconds when the document was compiled. This is used to compare against the `mtime` field of the file in order
	compiled_time: u64,

	/// Exported variables (serialized)
	variables: HashMap<VariableName, String>,
	/// Exported referenceable objects (serialized)
	references: HashMap<Refname, String>,

	/// Lists of other units this units depends on
	depends_on: Vec<String>,

	/// Compiler output
	output: String,
}

impl CompiledUnit {
	/// Initializes the tables related to compiled units:
	///  - `compiled_units` Store information about output and compilation time for units
	///  - `units_variables` Lists exported variables
	///  - `units_references` Lists exported references
	///  - `units_depends` Lists units dependencies
	pub fn init_tables<'con>(&self, con: MutexGuard<'con, Connection>) {
		con.execute(
			"CREATE TABLE IF NOT EXISTS compiled_units(
			input_file		TEXT PRIMARY KEY,
			output_file		TEXT NOT NULL,
			compiled_tile	INTEGER NOT NULL,
			output			TEXT NOT NULL,
		);

		CREATE TABLE IF NOT EXISTS units_variables(
			FOREIGN KEY(input_file) REFERENCES compiled_units(input_file)
			name			TEXT PRIMARY KEY,
			data			TEXT NOT NULL,
		);

		CREATE TABLE IF NOT EXISTS units_references(
			FOREIGN KEY(input_file) REFERENCES compiled_units(input_file)
			name			TEXT PRIMARY KEY,
			data			TEXT NOT NULL,
		);

		CREATE TABLE IF NOT EXISTS units_depends(
			depends_file	TEXT PRIMARY KEY
			FOREIGN KEY(input_file) REFERENCES compiled_units(input_file)
		);",
			(),
		);
	}

	pub fn new(output: CompilerOutput) -> Self { todo!() }

	/// Get dependencies of this unit
	pub fn get_dependent<'con>(&self, con: MutexGuard<'con, Connection>) -> Vec<String> {
		let mut stmt = con
			.prepare("SELECT input_file FROM units_depends WHERE depends_file = (?1)")
			.unwrap();
    	let rows = stmt
			.query_and_then([&self.input_file], |row| row.get::<_, String>(0))
			.unwrap();

		let mut result : Vec<String> = vec![];
		for row in rows
		{
			result.push(row.unwrap());
		}

		result
	}

	/// Saves this unit, overwriting previous iterations of it in the cache
	pub fn save_unit<'con>(&self, mut con: MutexGuard<'con, Connection>) {
		// Erase previous data
		con.execute(
			"
		DELETE FROM units_variables WHERE input_file = (?1);
		DELETE FROM units_references WHERE input_file = (?1);
		DELETE FROM units_depends WHERE input_file = (?1);",
		[&self.input_file],
		)
		.unwrap();

		// Store unit
		con.execute("INSERT OR REPLACE INTO compiled_units (input_file, output_file, compiled_tile, data) VALUES (?1, ?2, ?3, ?4);", 
			(&self.input_file,
			 &self.output_file,
			 self.compiled_time,
			 &self.output)
			).unwrap();

		let tr = con.transaction().unwrap();

		// Store variables
		for (name, data) in &self.variables {
			tr.execute("INSERT OR REPLACE INTO units_variables (input_file, name, data) VALUES (?1, ?2, ?3)", (&self.input_file, &name.0, data));
		}

		// Store references
		for (name, data) in &self.references {
			let Refname::Internal(name) = &name else { continue };
			tr.execute("INSERT OR REPLACE INTO units_references (input_file, name, data) VALUES (?1, ?2, ?3)", (&self.input_file, name, data));
		}

		// Store dependencies
		for depends_on in &self.depends_on {
			tr.execute("INSERT OR REPLACE INTO units_references (depends_file, input_file) VALUES (?1, ?2)", (depends_on, &self.input_file));
		}

		tr.finish();
	}
}
