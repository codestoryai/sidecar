use crate::agentic::tool::git::tools::{
    GitCommitTool, GitCreateBranchTool, GitPushTool, GitCreatePullRequestTool
};

impl ToolBroker {
    pub async fn new(
        llm_client: Arc<LLMBroker>,
        code_edit_broker: Arc<CodeEditBroker>,
        symbol_tracking: Arc<SymbolTrackerInline>,
        language_broker: Arc<TSLanguageParsing>,
        tool_broker_config: ToolBrokerConfiguration,
        fail_over_llm: LLMProperties,
    ) -> Self {
        let mut tools = HashMap::new();
        
        // ... existing tool registrations ...
        
        // Register git tools
        tools.insert(ToolType::GitCommit, Box::new(GitCommitTool) as Box<dyn Tool + Send + Sync>);
        tools.insert(ToolType::GitCreateBranch, Box::new(GitCreateBranchTool) as Box<dyn Tool + Send + Sync>);
        tools.insert(ToolType::GitPush, Box::new(GitPushTool) as Box<dyn Tool + Send + Sync>);
        tools.insert(ToolType::GitCreatePullRequest, Box::new(GitCreatePullRequestTool) as Box<dyn Tool + Send + Sync>);
        
        // ... rest of the implementation ...
    }
} 