use super::file::File;
use super::cursor::Cursor;

pub struct Token<'a>
{
    file: &'a File,
    start: usize,
    len: usize,
}

impl<'a> Token<'a>
{
    pub fn new(_file: &'a File, _start: usize, _len: usize) -> Token<'a>
    {
        Token {
            file: _file,
            start: _start,
            len: _len,
        }
    }

    pub fn from(cursor: &'a Cursor, mat: regex::Match<'a>) -> Token<'a>
    {
        Token {
            file: cursor.file,
            start: cursor.position,
            len: mat.len(),
        }
    }
}
