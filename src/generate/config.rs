use std::path::PathBuf;

pub struct GenerationConfig {
    pub source_path: PathBuf,
    pub header_path: PathBuf,
    pub dst_internals_path: PathBuf,
    pub dst_header_internals_file: PathBuf,
    pub use_anonymous_namespace: bool
}

impl GenerationConfig {
    pub fn namespace_cpp(&self, string: &str) -> String {
        if string.is_empty() {
            "GlobalNamespace".to_owned()
        } else {
            string.replace(['<', '>', '`', '/'], "_").replace('.', "::")
        }
    }
    pub fn full_name_cpp(&self, ns: &str, string: &str, nested: bool) -> String {
        let formatted_string = self.namespace_cpp(string);

        if ns.is_empty() && !nested {
            // essentially get GlobalNamespace
            let ns = self.namespace_cpp(ns);
            format!("{ns}::{formatted_string}")
        } else {
            formatted_string
        }
    }
    pub fn name_cpp(&self, string: &str) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.', '|'], "_")
    }
    pub fn generic_nested_name(&self, string: &str) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.', ':', '|'], "_")
    }
    pub fn namespace_path(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '/'], "_").replace('.', "/")
    }
    pub fn path_name(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '.', '/'], "_")
    }
}
