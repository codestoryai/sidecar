use serde::{Deserialize, Serialize};

pub struct Question {
    text: String,
    choices: Vec<Choice>,
}

impl Question {
    pub fn new(text: &str, choices: &[Choice]) -> Self {
        Self {
            text: text.to_string(),
            choices: choices.to_vec(),
        }
    }

    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }

    pub fn get_choice(&self, id: &str) -> Option<&Choice> {
        self.choices.iter().find(|choice| choice.id() == id)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_valid_choice(&self, choice_id: &str) -> bool {
        self.choices.iter().any(|choice| choice.id() == choice_id)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Choice {
    id: String,
    text: String,
}

impl Choice {
    pub fn new(id: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            text: text.to_string(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone)]
pub struct Answer {
    choice_id: String,
}

impl Answer {
    pub fn new(choice_id: String) -> Self {
        Self { choice_id }
    }

    pub fn choice_id(&self) -> &str {
        &self.choice_id
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "response")]
pub struct GenerateHumanQuestionResponse {
    pub text: String,
    #[serde(default)]
    pub choices: Choices,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Choices {
    #[serde(default)]
    pub choice: Vec<Choice>,
}

impl From<GenerateHumanQuestionResponse> for Question {
    fn from(response: GenerateHumanQuestionResponse) -> Self {
        Question {
            text: response.text,
            choices: response.choices.choice,
        }
    }
}
