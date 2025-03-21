use std::io::{self, Write};

struct State {
    last_input: String,
    last_result: String,
}

#[tokio::main]
async fn main() {
    println!("Welcome to the CLI Input Processor!");
    println!("Enter your input and press Enter to process it.");
    println!("Type 'exit' to quit the program.");

    let mut state = State {
        last_input: String::new(),
        last_result: String::new(),
    };

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim();

        if input.eq_ignore_ascii_case("exit") {
            println!("Goodbye!");
            break;
        }

        process_input(input, &mut state);
    }
}

fn process_input(input: &str, state: &mut State) {
    println!("Processing: {}", input);

    // Add your custom processing logic here
    // For now, we'll just convert the input to uppercase
    let result = input.to_uppercase();

    println!("Current result: {}", result);

    if !state.last_input.is_empty() {
        println!("Previous input: {}", state.last_input);
        println!("Previous result: {}", state.last_result);
    }

    // Update the state
    state.last_input = input.to_string();
    state.last_result = result;
}
