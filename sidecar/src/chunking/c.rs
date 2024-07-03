use crate::chunking::languages::TSLanguageConfig;

pub fn c_language_config() -> TSLanguageConfig {
    TSLanguageConfig {
        language_ids: &["C", "c"],
        file_extensions: &["c", "h"],
        grammar: tree_sitter_c::language,
        namespaces: vec![vec![
            "function",
            "variable",
            "struct",
            "enum",
            "union",
            "typedef",
            "field",
            "enumerator",
            "macro",
            "label",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect()],
        documentation_query: vec![
            "((comment) @comment
            (#match? @comment \"^///\")) @docComment".to_owned(),
        ],
        function_query: vec![
            "(function_definition
                declarator: (function_declarator
                    declarator: (identifier) @identifier
                    parameters: (parameter_list
                        (parameter_declaration
                            declarator: (identifier) @parameter.identifier
                            type: (type_identifier) @parameter.type
                        )*
                    ) @parameters
                )
                body: (compound_statement) @body
            ) @function".to_owned(),
        ],
        construct_types: vec![
            "translation_unit",
            "function_definition",
            "struct_specifier",
            "enum_specifier",
            "union_specifier",
            "type_definition",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        expression_statements: vec![
            "expression_statement",
            "declaration",
            "call_expression",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        class_query: vec![
            "(struct_specifier name: (type_identifier) @identifier) @class_declaration".to_owned(),
            "(union_specifier name: (type_identifier) @identifier) @class_declaration".to_owned(),
        ],
        r#type_query: vec![],
        namespace_types: vec![],
        hoverable_query: r#"
        [(identifier)
         (field_identifier)
         (type_identifier)] @hoverable
        "#
        .to_owned(),
        comment_prefix: "//".to_owned(),
        end_of_line: Some(";".to_owned()),
        import_identifier_queries: "(preproc_include) @import_type".to_owned(),
        block_start: Some("{".to_owned()),
        variable_identifier_queries: vec![
            "(declaration declarator: (identifier) @identifier)".to_owned(),
            "(call_expression function: (identifier) @identifier)".to_owned(),
        ],
        outline_query: Some(
            r#"
            (preproc_def) @definition.macro
            (preproc_function_def) @definition.macro
            (struct_specifier
                name: (type_identifier) @definition.class.name) @definition.class
            (enum_specifier
                name: (type_identifier) @definition.class.name) @definition.class
            (union_specifier
                name: (type_identifier) @definition.class.name) @definition.class
            (function_definition
                declarator: (function_declarator
                    declarator: (identifier) @function.name
                    parameters: (parameter_list) @parameters
                )
                body: (compound_statement) @function.body) @definition.function
            (type_definition
                declarator: (type_identifier) @definition.class.name) @definition.class"#
                .to_owned(),
        ),
        excluded_file_paths: vec![],
        language_str: "c".to_owned(),
    }
}