//! We want to read all the files which are present in the git respository

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

#[derive(Hash, PartialEq, Eq)]
enum FileType {
    Directory,
    File,
    NotTracked,
}

#[tokio::main]
async fn main() {
    let directory = Path::new("/Users/skcd/scratch/ide");
    let git = gix::open::Options::isolated()
        .filter_config_section(|_| false)
        .open(directory)
        .expect("to work");

    let local_git = git.to_thread_local();
    let mut head = local_git.head().expect("get this");
    let trees = vec![(
        true,
        "HEAD".to_owned(),
        head.peel_to_commit_in_place()
            .expect("to work")
            .tree()
            .expect("to work"),
    )];

    let directory_ref: &Path = directory.as_ref();

    let entries = trees
        .into_iter()
        .flat_map(|(is_head, branch, tree)| {
            let files = tree.traverse().breadthfirst.files().unwrap().into_iter();

            files.map(move |entry| {
                let strpath = String::from_utf8_lossy(entry.filepath.as_ref());
                let full_path = directory_ref.join(strpath.as_ref());
                (
                    is_head,
                    branch.clone(),
                    full_path.to_string_lossy().to_string(),
                    entry.mode,
                    entry.oid,
                )
            })
        })
        .fold(
            HashMap::new(),
            |mut acc, (is_head, branch, file, mode, oid)| {
                let kind = if mode.is_tree() {
                    FileType::Directory
                } else if mode.is_blob() {
                    FileType::File
                } else {
                    FileType::NotTracked
                };

                let branches = acc.entry((file, kind, oid)).or_insert_with(BTreeSet::new);
                if is_head {
                    branches.insert("HEAD".to_string());
                }

                branches.insert(branch);
                acc
            },
        );

    // Now we want to read the content of all the files in parallel
    {
        let start_time = std::time::Instant::now();
        use rayon::prelude::*;
        let _values = entries
            .into_par_iter()
            .filter_map(|((_path, kind, oid), _)| {
                let git = git.to_thread_local();
                let Ok(Some(object)) = git.try_find_object(oid) else {
                    return None;
                };

                let entry = match kind {
                    FileType::File => {
                        let buffer = String::from_utf8_lossy(&object.data).to_string();
                        Some(buffer)
                    }
                    FileType::Directory => None,
                    FileType::NotTracked => None,
                };
                entry
            })
            .collect::<Vec<_>>();
        println!("Time taken: {}", start_time.elapsed().as_micros());
    }
}
