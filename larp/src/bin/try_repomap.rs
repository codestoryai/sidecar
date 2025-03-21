use sidecar::{
    agentic::tool::lsp::list_files::list_files,
    repomap::{tag::TagIndex, types::RepoMap},
};
use std::path::Path;

#[tokio::main]
async fn main() {
    // let full_path = Path::new("/Users/zi/codestory/sidecar/sidecar/src");
    // let tag_index = TagIndex::from_path(full_path).await;
    let _files = vec![
        "/Users/skcd/scratch/sidecar/sidecar/src/repomap/tag.rs".to_owned(),
        "/Users/skcd/scratch/sidecar/sidecar/src/repomap/types.rs".to_owned(),
    ];

    let root_path = Path::new("/Users/skcd/scratch/sidecar/sidecar/src/agentic/tool");

    // read all the files which are part of this directory
    // we also want to make sure that we can set the limit on the number of files
    // which are required
    let directory_files_stream = list_files(&root_path, true, 250)
        .0
        .into_iter()
        .filter_map(|file_path| {
            if file_path.is_dir() {
                None
            } else {
                Some(file_path)
            }
        })
        .map(|file_path| file_path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    // it does not take as input the directories over here while creating a repo map
    let tag_index = TagIndex::from_files(root_path, directory_files_stream).await;

    let repomap = RepoMap::new().with_map_tokens(5_000);

    let repomap_string = repomap.get_repo_map(&tag_index).await.unwrap();

    println!("Getting repo map");
    println!("========================================================");
    println!("{}", repomap_string);
    println!("========================================================");
}
