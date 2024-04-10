use std::path::Path;

#[derive(Debug)]
pub struct File
{
    pub path: String,
}

impl File
{
    pub fn new(_path: String) -> File
    {
        File {
            path: _path,
        }
    }
}
