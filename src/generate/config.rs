use std::path::PathBuf;

pub struct GenerationConfig {
    pub source_path: PathBuf,
    pub header_path: PathBuf,
}

impl GenerationConfig {
    pub fn namespace_cpp(&self, string: String) -> String {
        string.replace(['<', '>', '`', '/'], "_").replace('.', "::")
    }
    pub fn name_cpp(&self, string: String) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.'], "_")
    }
    pub fn namespace_path(&self, string: String) -> String {
        string.replace(['<', '>', '`', '/'], "_").replace('.', "/")
    }
    pub fn path_name(&self, string: String) -> String {
        string.replace(['<', '>', '`', '.', '/'], "_")
    }
}
