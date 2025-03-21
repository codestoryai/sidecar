use super::{
    error::CommunicationError,
    human::Communicator,
    qa::{Answer, Question},
};

use std::io::{self, Write};

pub struct CliCommunicator;

impl Communicator for CliCommunicator {
    fn ask_question(&self, question: &Question) -> Result<Answer, CommunicationError> {
        println!("{}", question.text());
        for choice in question.choices() {
            println!("  [{}] {}", choice.id(), choice.text());
        }

        // Flush stdout to ensure the question is displayed immediately
        io::stdout().flush()?;

        loop {
            // Read user input
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            // Trim whitespace
            let choice_id = input.trim();

            // Validate the answer
            if question.is_valid_choice(choice_id) {
                let answer = Answer::new(choice_id.to_string());
                println!(
                    "You selected #{}: {}",
                    answer.choice_id(),
                    question
                        .get_choice(answer.choice_id())
                        .map_or("Unable to retrieve choice text", |choice| choice.text())
                );
                return Ok(answer);
            } else {
                println!("Invalid choice. Please try again.");
            }
        }
    }
}
