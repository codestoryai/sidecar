use std::{sync::Arc, time::Instant};

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use async_trait::async_trait;

use crate::agentic::{symbol::identifier::LLMProperties, tool::kw_search::types::KeywordsReply};

use super::{
    tool::{KeywordSearch, KeywordSearchQuery},
    types::KeywordsReplyError,
};

pub struct GoogleStudioKeywordSearch {
    llm_client: Arc<LLMBroker>,
    _fail_over_llm: LLMProperties,
}

impl GoogleStudioKeywordSearch {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            _fail_over_llm: fail_over_llm,
        }
    }

    pub fn system_message_for_keyword_search(&self, _request: &KeywordSearchQuery) -> String {
        format!(
            r#"You are a keyword search expert.

You will be provided with a user_query and a repository name.

You will return a list of key words that will help you achieve the user_query. Focus on keywords that may be unique or idiosyncratic to the user_query's problem domain.

Avoid returning keywords that may be too generic or common e.g. "new", "add", "remove", "update", "fix", "bug", "error", "issue", "problem", "solution", etc.

Here is an example:

<user_query>
"
@ (__matmul__) should fail if one argument is not a matrix
```
>>> A = Matrix([[1, 2], [3, 4]])
>>> B = Matrix([[2, 3], [1, 2]])
>>> A@B
Matrix([
[ 4,  7],
[10, 17]])
>>> 2@B
Matrix([
[4, 6],
[2, 4]])
```

Right now `@` (`__matmul__`) just copies `__mul__`, but it should actually only work if the multiplication is actually a matrix multiplication. 

This is also how NumPy works

```
>>> import numpy as np
>>> a = np.array([[1, 2], [3, 4]])
>>> 2*a
array([[2, 4],
        [6, 8]])
>>> 2@a
Traceback (most recent call last):
    File ""<stdin>"", line 1, in <module>
ValueError: Scalar operands are not allowed, use '*' instead
```
"
</user_query>

<repository>
sympy/sympy
</repository>

And the response:
<keywords>
__matmul__
matrix
__mul__
numpy
scalar
</keywords>


Respond in the following format:

<keywords>
keyword
keyword
keyword
</keywords>"#
        )
    }

    pub fn user_message_for_keyword_search(&self, request: &KeywordSearchQuery) -> String {
        format!(
            r#"<user_query>
{}
</user_query>

<repository>
{}
</repository>"#,
            request.user_query(),
            request.repo_name()
        )
    }
}

#[async_trait]
impl KeywordSearch for GoogleStudioKeywordSearch {
    async fn get_keywords(
        &self,
        request: &KeywordSearchQuery,
    ) -> Result<KeywordsReply, KeywordsReplyError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.llm().clone();
        let provider = request.provider().clone();
        let api_keys = request.api_keys().clone();

        let system_message =
            LLMClientMessage::system(self.system_message_for_keyword_search(&request));
        let user_message = LLMClientMessage::user(self.user_message_for_keyword_search(&request));
        let messages = LLMClientCompletionRequest::new(
            model,
            vec![system_message.clone(), user_message.clone()],
            0.2,
            None,
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();

        let start = Instant::now();

        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                messages,
                provider,
                vec![
                    ("event_type".to_owned(), "keyword_search".to_owned()),
                    ("root_id".to_owned(), root_request_id),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;

        println!("keyword_search::response_time({:?})", start.elapsed());

        let parsed_response = KeywordsReply::parse_response(response.answer_up_until_now());

        match parsed_response {
            Ok(parsed_response) => Ok(parsed_response),
            Err(e) => Err(e),
        }
    }
}
