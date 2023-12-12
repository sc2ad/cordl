#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash, Clone)]
pub struct NameComponents {
    pub namespace: Option<String>,
    pub declaring_types: Option<Vec<String>>,
    pub name: String,
    pub generics: Option<Vec<String>>,
    pub is_pointer: bool,
}

impl NameComponents {
    // TODO: Add setting for adding :: prefix
    // however, this cannot be allowed in all cases
    pub fn combine_all(&self) -> String {
        let combined_declaring_types = self.declaring_types.as_ref().map(|d| d.join("::"));

        // will be empty if no namespace or declaring types
        let prefix = combined_declaring_types
            .as_ref()
            .or(self.namespace.as_ref())
            .map(|s| format!("{s}::"))
            .unwrap_or_default();

        let mut completed = format!("{prefix}{}", self.name);

        if let Some(generics) = &self.generics {
            completed = format!("{completed}<{}>", generics.join(","));
        }

        if self.is_pointer {
            completed = format!("{completed}*")
        }

        completed
    }

    pub fn ref_generics(self) -> Self {
        Self {
            generics: self
                .generics
                .map(|opt| opt.into_iter().map(|_| "void*".to_string()).collect()),
            ..self
        }
    }

    pub fn remove_generics(self) -> Self {
        Self {
            generics: None,
            ..self
        }
    }

    pub fn as_pointer(&self) -> Self {
        Self {
            is_pointer: true,
            ..self.clone()
        }
    }
    pub fn remove_pointer(&self) -> Self {
        Self {
            is_pointer: false,
            ..self.clone()
        }
    }

    /// just cpp name with generics
    pub fn formatted_name(&self, include_generics: bool) -> String {
        if let Some(generics) = &self.generics && include_generics {
            format!("{}<{}>", self.name, generics.join(","))
        } else {
            self.name.to_string()
        }
    }
}

impl From<String> for NameComponents {
    fn from(value: String) -> Self {
        Self {
            name: value,
            ..Default::default()
        }
    }
}
