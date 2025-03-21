use super::{
    error::CommunicationError,
    qa::{Answer, Question},
};

pub enum CommunicationInterface {
    Cli,
}

pub trait Communicator {
    fn ask_question(&self, question: &Question) -> Result<Answer, CommunicationError>;
}

pub struct Human<T: Communicator> {
    communicator: T,
}

impl<T: Communicator> Human<T> {
    pub fn new(communicator: T) -> Self {
        Self { communicator }
    }

    pub fn ask(&self, question: &Question) -> Result<Answer, CommunicationError> {
        self.communicator.ask_question(question)
    }
}
