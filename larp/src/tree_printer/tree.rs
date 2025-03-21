use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
};

use ignore::Walk;

pub struct TreePrinter {}

impl TreePrinter {
    pub fn to_string_stacked(root: &Path) -> io::Result<String> {
        let mut stack = VecDeque::new();
        stack.push_back((PathBuf::from(root), Vec::new()));
        let mut output: Vec<String> = vec![];

        while let Some((path, prefix_data)) = stack.pop_back() {
            for &is_last in &prefix_data[..prefix_data.len().saturating_sub(1)] {
                if output.is_empty() {
                    output.push("".to_owned());
                }
                if let Some(last_entry) = output.last_mut() {
                    if is_last {
                        *last_entry = last_entry.to_owned() + "    ";
                    } else {
                        *last_entry = last_entry.to_owned() + "│   ";
                    }
                }
            }

            if let Some(&is_last) = prefix_data.last() {
                if output.is_empty() {
                    output.push("".to_owned());
                }
                if let Some(last_entry) = output.last_mut() {
                    if is_last {
                        *last_entry = last_entry.to_owned() + "└── ";
                    } else {
                        *last_entry = last_entry.to_owned() + "├── ";
                    }
                }
            }

            if output.is_empty() {
                output.push("".to_owned());
            }
            if let Some(last_entry) = output.last_mut() {
                *last_entry = last_entry.to_owned()
                    + &path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
            }
            output.push("".to_owned());

            if path.is_dir() {
                let mut entries: Vec<_> = Walk::new(&path)
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.depth() == 1)
                    .map(|entry| entry.into_path())
                    .collect();

                entries.sort();
                entries.reverse();

                for (i, entry) in entries.into_iter().enumerate() {
                    let is_last = i == 0;
                    let mut new_prefix_data = prefix_data.clone();
                    new_prefix_data.push(is_last);
                    stack.push_back((entry, new_prefix_data));
                }
            }
        }

        Ok(output.join("\n"))
    }
}
