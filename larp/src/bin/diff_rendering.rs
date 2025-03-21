use similar::TextDiff;

fn main() {
    let diff = TextDiff::from_lines(
        r#"Hello World
This is the second line.
This is the third."#,
        r#"Hallo Welt
This is the second line.
This is life.
Moar and more"#,
    );
    let unified_diff = diff.unified_diff();

    for change in unified_diff.iter_hunks() {
        println!("{}", change);
        println!("======================");
    }
}
