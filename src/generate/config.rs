use std::path::PathBuf;

pub struct GenerationConfig {
    pub source_path: PathBuf,
    pub header_path: PathBuf,
}

impl GenerationConfig {
    pub fn namespace_cpp(&self, string: &str) -> String {
        if string.is_empty() {
            "GlobalNamespace".to_owned()
        } else {
            string.replace(['<', '>', '`', '/'], "_").replace('.', "::")
        }
    }
    pub fn name_cpp(&self, string: &str) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.'], "_")
    }
    pub fn generic_nested_name(&self, string: &str) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.', ':'], "_")
    }
    pub fn namespace_path(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '/'], "_").replace('.', "/")
    }
    pub fn path_name(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '.', '/'], "_")
    }
}
