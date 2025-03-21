use std::collections::HashMap;

pub struct FileAnalyser {
    file_collections: Vec<Vec<String>>,
}

impl FileAnalyser {
    pub fn new() -> Self {
        FileAnalyser {
            file_collections: Vec::new(),
        }
    }

    pub fn add_file_collection(&mut self, file_collection: Vec<String>) {
        self.file_collections.push(file_collection);
    }

    pub fn analyze_top_occurrences(&self, top_n: usize) -> Vec<(String, usize)> {
        let mut occurrences: HashMap<String, usize> = HashMap::new();

        for file_collection in &self.file_collections {
            for file in file_collection {
                *occurrences.entry(file.clone()).or_insert(0) += 1;
            }
        }

        let mut sorted_occurrences: Vec<_> = occurrences.into_iter().collect();
        sorted_occurrences.sort_by(|a, b| b.1.cmp(&a.1));

        sorted_occurrences.into_iter().take(top_n).collect()
    }
}
