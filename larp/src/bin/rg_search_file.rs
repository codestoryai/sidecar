use std::process::Stdio;

use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

/// Define the command-line arguments
#[derive(Parser, Debug)]
#[command(author = "skcd", version = "1.0", about = "RG command runner")]
struct CliArgs {
    #[arg(long)]
    directory_path: String,

    #[arg(long)]
    regex_pattern: String,
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();
    let regex_pattern = &args.regex_pattern;
    // let file_pattern = "*".to_owned();
    let rg_args = vec![
        "--json",
        // enables lookaround
        "--pcre2",
        "-e",
        regex_pattern,
        // "--glob",
        // &file_pattern,
        "--context",
        "1",
        // do not enable multiline over here, from the docs:
        // https://gist.github.com/theskcd/a6369001b3ea3c0212bbc88d8a74211f from
        // rg --help | grep multiline
        // "--multiline",
        // &args.directory_path,
    ];

    let mut child = Command::new("rg")
        .args(&rg_args)
        .stdout(Stdio::piped())
        // close stdin so rg does not wait for input from the stdin fd
        .stdin(Stdio::null())
        // set the current directory for the command properly
        .current_dir(args.directory_path.to_owned())
        .spawn()
        .expect("to work");

    // now we can read the output from the child line by line and parse it out properly
    let stdout = child.stdout.take();
    if let None = stdout {
        println!("stdout is empty over here");
        return;
    }

    let stdout = stdout.expect("Failed to capture stdout");
    let reader = BufReader::new(stdout).lines();

    let mut output = String::new();
    let mut line_count = 0;
    let max_lines = 500 * 5;

    tokio::pin!(reader);

    while let Some(line) = reader.next_line().await.expect("to work") {
        if line_count >= max_lines {
            break;
        }
        output.push_str(&line);
        output.push('\n');
        line_count += 1;
    }

    println!("{:?}", &output);

    // rip grep args

    println!("search_files::args::({:?})", rg_args);
}
