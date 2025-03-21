use serde::{Deserialize, Serialize};
use serde_xml_rs::from_str;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Item {
    name: String,
    source: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "name")]
pub struct SymbolName {
    #[serde(rename = "$value")]
    name: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "thinking")]
pub struct SymbolThinking {
    #[serde(rename = "$value")]
    thinking: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol")]
pub struct Symbol {
    name: String,
    thinking: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "step_list")]
pub struct StepListItem {
    name: String,
    step: Vec<String>,
    #[serde(default)]
    new: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol_list")]
pub struct SymbolList {
    #[serde(rename = "$value")]
    symbol_list: Vec<Symbol>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "step_by_step")]
pub struct StepList {
    #[serde(rename = "$value")]
    steps: Vec<StepListItem>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct Reply {
    symbol_list: SymbolList,
    // #[serde(rename = "step_by_step")]
    step_by_step: StepList,
}

fn main() {
    let src = r#"
    <reply>
    <symbol_list>
    <symbol>
    <name>
    ReRank
    </name>
    <thinking>
    The ReRank trait is the main interface for implementing different reranking algorithms. We will need to add a new implementation of this trait for the OpenAI reranker.
    </thinking>
    </symbol>
    <symbol>
    <name>
    ReRankBroker
    </name>
    <thinking>
    The ReRankBroker is responsible for managing the different rerankers. We will need to add the new OpenAI reranker to this broker.
    </thinking>
    </symbol>
    <symbol>
    <name>
    OpenAIReRank
    </name>
    <new>true</new>
    <thinking>
    We will need to create a new struct called OpenAIReRank that implements the ReRank trait.
    </thinking>
    </symbol>
    </symbol_list>
    
    <step_by_step>
    <step_list>
    <name>
    OpenAIReRank
    </name>
    <new>true</new>
    <step>
    Create a new struct called OpenAIReRank that implements the ReRank trait. This struct will need to have a reference to the LLMBroker, similar to the AnthropicReRank implementation.
    </step>
    <step>
    Implement the rerank method for the OpenAIReRank struct. This method will need to use the OpenAI LLM client to perform the reranking operation.
    </step>
    </step_list>
    
    <step_list>
    <name>
    ReRankBroker
    </name>
    <step>
    Update the ReRankBroker struct to include the new OpenAIReRank implementation. Add a new entry to the rerankers HashMap, mapping the LLMType for the OpenAI model to the new OpenAIReRank implementation.
    </step>
    </step_list>
    </step_by_step>
    </reply>
"#;
    let checking_symbol: Reply = from_str(&src).unwrap();
    println!("{:?}", checking_symbol);
}
