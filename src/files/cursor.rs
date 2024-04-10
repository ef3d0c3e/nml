use super::file::File;

#[derive(Debug)]
pub struct Cursor<'a>
{
    pub file: &'a File,
    pub content: String,
    pub position: usize,
}

impl<'a> Cursor<'a>
{
    pub fn new(_file: &'a File) -> Result<Cursor<'a>, std::io::Error>
    {
        let _content = match std::fs::read_to_string(&_file.path)
        {
            Ok(content) => content,
            Err(error) => return Err(error),
        };

        Ok(Cursor
        {
            file: _file,
            content: _content,
            position: 0usize,
        })
    }
}
