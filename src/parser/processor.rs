
pub struct FileProcessor
{
	files: Vec<String>,
}

impl FileProcessor {
	pub fn new(files: Vec<String>) -> Self {
		Self {
			files
		}
	}
}
