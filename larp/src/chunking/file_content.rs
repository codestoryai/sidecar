//! Uses the full file with no tree sitter queries
//! This is a big hack to literally give back a single node called File

use super::languages::TSLanguageConfig;

pub fn file_content_language_config() -> TSLanguageConfig {
    TSLanguageConfig {
        language_ids: &["*"],
        file_extensions: &["*"],
        // load the go one by default, ideally grammar will be Option<Parser>
        // over here
        grammar: tree_sitter_go::language,
        namespaces: vec![],
        documentation_query: vec!["".to_owned()],
        function_query: vec![],
        construct_types: vec![],
        expression_statements: vec![],
        class_query: vec![],
        r#type_query: vec![],
        namespace_types: vec![],
        hoverable_query: "".to_owned(),
        comment_prefix: "".to_owned(),
        end_of_line: None,
        // TODO(skcd): Finish this up properly
        import_identifier_queries: "".to_owned(),
        block_start: None,
        variable_identifier_queries: vec!["".to_owned()],
        outline_query: Some("".to_owned()),
        excluded_file_paths: vec![],
        language_str: "*".to_owned(),
        object_qualifier: "".to_owned(),
        file_definitions_query: "".to_owned(),
        required_parameter_types_for_functions: "".to_owned(),
        function_call_path: None,
    }
}
