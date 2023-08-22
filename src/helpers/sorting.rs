use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
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

    // Topologically sorts the dependency graph, handling cyclic dependencies
    pub fn topological_sort(&self) -> Vec<&'a A> {
        let mut visited = HashSet::new();
        let mut stack = VecDeque::new();

        let mut sort_fn = self.sorting;

        for dependent in self.dependencies.keys().sorted_by(|a, b| sort_fn(*a, *b)) {
            if !visited.contains(dependent) {
                self.topological_sort_recurse(dependent, &mut visited, &mut stack);
            }
        }

        stack.into_iter().collect_vec()
    }

    // Perform a recursive topological sort for the given dependency, stack, and visited collection
    fn topological_sort_recurse(
        &self,
        main: &'a A,
        visited: &mut HashSet<&'a A>,
        stack: &mut VecDeque<&'a A>,
    ) {
        if visited.contains(main) {
            return;
        }

        visited.insert(main);

        let mut sort_fn = self.sorting;

        if let Some(dependencies) = self.dependencies.get(main) {
            let mut sorted_dependencies: Vec<_> = dependencies.iter().collect();
            sorted_dependencies.sort_by(|a, b| (sort_fn)(*a, *b));

            for dependency in sorted_dependencies {
                self.topological_sort_recurse(dependency, visited, stack);
            }
        }

        stack.push_back(main);
    }
}
