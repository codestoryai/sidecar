// Here we are going to use the cohere reranking to rerank the snippets
// according to their relevance to the user query

use std::collections::HashMap;

use llm_client::clients::codestory::CodeStoryClient;
use llm_prompts::reranking::types::CodeSpanDigest;

#[derive(Debug, serde::Serialize)]
struct ReRankQuery {
    query: String,
    documents: Vec<String>,
    top_n: usize,
}

#[derive(Debug, serde::Deserialize)]
struct ReRankResponse {
    results: Vec<ReRankResult>,
}

#[derive(Debug, serde::Deserialize)]
struct ReRankResult {
    index: usize,
}

// TODO(skcd): Figure out the right layer to put this in, it feels awkward
// to have this hanging as a function
pub async fn rerank_snippets(
    codestory_client: &CodeStoryClient,
    snippets: Vec<CodeSpanDigest>,
    query: &str,
) -> Vec<CodeSpanDigest> {
    let rerank_endpoint = codestory_client.rerank_endpoint();
    let client = codestory_client.client();
    let rerank_query = ReRankQuery {
        query: query.to_owned(),
        documents: snippets
            .iter()
            .map(|digest| digest.data().to_owned())
            .collect(),
        top_n: snippets.len(),
    };
    let response = client
        .post(rerank_endpoint)
        .json(&rerank_query)
        .send()
        .await;
    match response {
        Ok(response) => {
            // get the response string
            let response_string = response.text().await;
            match response_string {
                Ok(response_string) => {
                    // now we parse it out
                    let response = serde_json::from_str::<ReRankResponse>(&response_string);
                    match response {
                        Ok(response) => {
                            let ranking = response
                                .results
                                .into_iter()
                                .map(|rerank_result| rerank_result.index)
                                .collect::<Vec<_>>();
                            let mut snippet_mapping = snippets
                                .into_iter()
                                .enumerate()
                                .collect::<HashMap<usize, CodeSpanDigest>>();
                            let mut reranked_snippets = vec![];
                            for index in ranking {
                                if let Some(snippet) = snippet_mapping.remove(&index) {
                                    reranked_snippets.push(snippet);
                                }
                            }
                            reranked_snippets
                        }
                        Err(_) => snippets,
                    }
                }
                Err(_) => snippets,
            };
            unimplemented!();
        }
        Err(_) => snippets,
    }
}
