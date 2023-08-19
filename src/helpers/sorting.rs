use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
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
    A: Eq + Hash,
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
}
