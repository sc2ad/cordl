use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

use itertools::Itertools;

// Define a dependency graph structure to store dependency relationships
pub struct DependencyGraph<'a, A, F> {
    dependencies: HashMap<&'a A, HashSet<&'a A>>,
    sorting: F,
}

impl<'a, A, F> DependencyGraph<'a, A, F>
where
    A: Eq + Hash + Debug,
    F: FnMut(&&'a A, &&'a A) -> Ordering + Copy,
{
    // Initialize a new empty dependency graph
    pub fn new(sorting: F) -> Self {
        Self {
            dependencies: HashMap::new(),
            sorting,
        }
    }

    // Add a dependency relationship to the graph
    pub fn add_root_dependency(&mut self, dependent: &'a A) -> bool {
        self.dependencies
            .try_insert(dependent, HashSet::new())
            .is_ok()
    }

    // Add a dependency relationship to the graph
    pub fn add_dependency(&mut self, dependent: &'a A, dependency: &'a A) {
        self.dependencies
            .entry(dependent)
            .or_insert(HashSet::new())
            .insert(dependency);
    }

    // Perform a topological sort with deterministic sorting
    pub fn topological_sort(
        &self,
        object: &'a A,
        visited: &mut HashSet<&'a A>,
        result: &mut Vec<&'a A>,
    ) {
        visited.insert(object.clone());

        if let Some(dependencies) = self.dependencies.get(&object) {
            // Collect dependencies and sort them alphabetically
            let sorted_dependencies: Vec<_> = dependencies
                .iter()
                .cloned()
                .sorted_by(self.sorting)
                .collect();

            // Recursively process sorted dependencies
            for dependency in sorted_dependencies {
                if !visited.contains(&dependency) {
                    self.topological_sort(dependency.clone(), visited, result);
                }
            }
        }

        // Add the current object to the result
        result.push(object);
    }
    pub fn get_dependencies_sorted(&self) -> Vec<&'a A> {
        // Identify root objects (objects with no incoming dependencies)
        let mut root_objects = HashSet::new();
        let mut all_objects = HashSet::new();

        // Collect all objects and their dependencies
        for (dependent, dependencies) in &self.dependencies {
            all_objects.insert(dependent);
            for dependency in dependencies {
                all_objects.insert(dependency);
            }
        }

        // Find root objects
        for object in all_objects.iter() {
            let has_incoming_dependencies = self
                .dependencies
                .values()
                .any(|deps| deps.contains(*object));
            if !has_incoming_dependencies {
                root_objects.insert(object);
            }
        }

        // Perform reverse topological sort
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        let mut sort_fn = self.sorting;

        for root_object in root_objects.iter().sorted_by(|a, b| (sort_fn)(**a, **b)) {
            self.topological_sort(root_object, &mut visited, &mut result);
        }

        result
    }

    pub fn get_reverse_dependencies_sorted(&self) -> Vec<&'a A> {
        // Identify leaf objects (objects with no outgoing dependencies)
        let mut leaf_objects = HashSet::new();
        let mut all_objects = HashSet::new();

        // Collect all objects and their dependents
        for (dependency, dependents) in &self.dependencies {
            all_objects.insert(dependency);
            for dependent in dependents {
                all_objects.insert(dependent);
            }
        }

        // Find leaf objects
        for object in all_objects.iter() {
            let has_outgoing_dependencies = self
                .dependencies
                .get(*object)
                .map_or(false, |dependents| !dependents.is_empty());
            if !has_outgoing_dependencies {
                leaf_objects.insert(object);
            }
        }

        // Perform reverse topological search
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        let mut sort_fn = self.sorting;

        for leaf_object in leaf_objects.iter().sorted_by(|a, b| (sort_fn)(**a, **b)) {
            self.reverse_topological_search(leaf_object, &mut visited, &mut result);
        }

        result
    }

    // Add a function for reverse topological search
    pub fn reverse_topological_search(
        &self,
        object: &'a A,
        visited: &mut HashSet<&'a A>,
        result: &mut Vec<&'a A>,
    ) {
        visited.insert(object.clone());

        if let Some(dependents) = self.dependencies.get(object) {
            let sorted_dependents: Vec<_> =
                dependents.iter().cloned().sorted_by(self.sorting).collect();

            for dependent in sorted_dependents {
                if !visited.contains(&dependent) {
                    self.reverse_topological_search(dependent.clone(), visited, result);
                }
            }
        }

        result.push(object);
    }

    // Add a function to generate a sorted graph representation
    pub fn generate_sorted_graph_representation(&self) -> Vec<String> {
        let mut sorted_representation = Vec::new();

        for (dependent, dependencies) in &self.dependencies {
            let dependent_str = format!("{:?}", dependent);
            let dependencies_str: Vec<String> = dependencies
                .iter()
                .map(|dep| format!("{:?}", dep))
                .collect();

            let mut dependency_line = String::new();
            if !dependencies_str.is_empty() {
                dependency_line.push_str(&format!("{} --> ", dependent_str));
                dependency_line.push_str(&dependencies_str.join(" | "));
            } else {
                dependency_line.push_str(&format!("{} -->", dependent_str));
            }

            sorted_representation.push(dependency_line);
        }

        sorted_representation
    }

    pub fn generate_reverse_topological_visual_representation(&self) -> String {
        let mut visual_representation = String::new();

        // Create a recursive traversal function
        fn traverse<'a, A, F>(
            graph: &DependencyGraph<'a, A, F>,
            dependent: &'a A,
            level: usize,
            representation: &mut String,
        ) where
            A: Eq + Hash + Debug,
            F: FnMut(&&'a A, &&'a A) -> Ordering + Copy,
        {
            let mut sort_fn = graph.sorting;

            representation.push_str(&format!("{:width$}", "", width = level * 4));
            representation.push_str(&format!("{:?} --> ", dependent));

            if let Some(dependencies) = graph.dependencies.get(dependent) {
                let sorted_dependencies: Vec<_> = dependencies
                    .iter()
                    .sorted_by(|a, b| (sort_fn)(*a, *b))
                    .collect();

                if !sorted_dependencies.is_empty() {
                    let mut dep_string = String::new();
                    for (index, dependency) in sorted_dependencies.iter().enumerate() {
                        if index > 0 {
                            dep_string.push_str(" | ");
                        }
                        dep_string.push_str(&format!("{:?}", dependency));
                    }
                    representation.push_str(&dep_string);
                }
                representation.push('\n');

                for dependency in sorted_dependencies {
                    traverse(graph, dependency, level + 1, representation);
                }
            }
        }

        let mut sort_fn = self.sorting;

        for dependent in self
            .dependencies
            .keys()
            .filter(|&&dep| !self.dependencies.values().any(|deps| deps.contains(dep)))
            .sorted_by(|a, b| (sort_fn)(*a, *b))
        {
            traverse(self, dependent, 0, &mut visual_representation);
        }

        visual_representation
    }
}
