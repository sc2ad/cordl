#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash, Clone)]
pub struct NameComponents {
    pub namespace: String,
    pub declaring_types: Vec<String>,
    pub name: String,
    pub generics: Option<Vec<String>>,
}

impl NameComponents {
    // TODO: Add setting for adding :: prefix
    // however, this cannot be allowed in all cases
    pub fn combine_all(&self, include_generics: bool) -> String {
        let combined_namespace = match self.declaring_types.is_empty() {
            true => self.namespace.to_string(),
            false => format!("{}::{}", self.namespace, self.declaring_types.join("::")),
        };

        let finished = format!("{combined_namespace}::{}", self.name);

        if include_generics && let Some(generics) = &self.generics {
            format!("{finished}<{}>", generics.join(", "))
        } else {
            finished
        }
    }

    pub fn formatted_name(&self, include_generics: bool) -> String {
        if let Some(generics) = &self.generics && include_generics {
            format!("{}<{}>", self.name, generics.join(", "))
        } else {
            self.name.to_string()
        }
    }
}
