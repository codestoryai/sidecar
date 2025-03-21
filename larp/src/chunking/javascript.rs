use crate::chunking::languages::TSLanguageConfig;

pub fn javascript_language_config() -> TSLanguageConfig {
    TSLanguageConfig {
        language_ids: &["Javascript", "JSX", "javascript", "jsx"],
        file_extensions: &["js", "jsx"],
        grammar: tree_sitter_javascript::language,
        namespaces: vec![vec![
            //variables
            "constant",
            "variable",
            "property",
            "function",
            "method",
            "generator",
            // types
            "class",
            // misc.
            "label",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect()],
        documentation_query: vec!["((comment) @comment
        (#match? @comment \"^\\\\/\\\\*\\\\*\")) @docComment"
            .to_owned()],
        function_query: vec!["[
            (function_declaration
              name: (identifier)? @identifier
              parameters: (formal_parameters
                          (identifier) @parameter.identifier
                     )? @parameters
              body: (statement_block
                        (lexical_declaration
                          (variable_declarator
                            name: (identifier) @variable.name
                          )
                        )*
                      )? @body)
            (generator_function
              name: (identifier)? @identifier
              parameters: (formal_parameters
                          (identifier) @parameter.identifier
                     )? @parameters
              body: (statement_block
                        (lexical_declaration
                          (variable_declarator
                            name: (identifier) @variable.name
                          )
                        )*
                      )? @body)
            (generator_function_declaration
              name: (identifier)? @identifier
              parameters: (formal_parameters
                          (identifier) @parameter.identifier
                     )? @parameters
              body: (statement_block
                        (lexical_declaration
                          (variable_declarator
                            name: (identifier) @variable.name
                          )
                        )*
                      )? @body)
            (method_definition
              name: (property_identifier)? @identifier
              parameters: (formal_parameters
                          (identifier) @parameter.identifier
                     )? @parameters
              body: (statement_block
                        (lexical_declaration
                          (variable_declarator
                            name: (identifier) @variable.name
                          )
                        )*
                      )? @body)
            (arrow_function
              parameters: (formal_parameters
                          (identifier) @parameter.identifier
                     )? @parameters
              body: (statement_block
                        (lexical_declaration
                          (variable_declarator
                            name: (identifier) @variable.name
                          )
                        )*
                      )? @body)
          ] @function"
            .to_owned()],
        construct_types: vec![
            "program",
            "class_declaration",
            "function_declaration",
            "function",
            "method_definition",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        expression_statements: vec![
            "call_expression",
            "expression_statement",
            "variable_declaration",
            "public_field_definition",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        class_query: vec![
            "[(class_declaration name: (identifier)? @identifier)] @class_declaration".to_owned(),
        ],
        r#type_query: vec![],
        namespace_types: vec![],
        hoverable_query: r#"
        [(identifier)
         (property_identifier)
         (shorthand_property_identifier)
         (shorthand_property_identifier_pattern)
         (private_property_identifier)
         (statement_identifier)] @hoverable
        "#
        .to_owned(),
        comment_prefix: "//".to_owned(),
        end_of_line: Some(";".to_owned()),
        // TODO(skcd): Finish this up properly
        import_identifier_queries: r#"
        "#
        .to_owned(),
        block_start: Some("{".to_owned()),
        variable_identifier_queries: vec![
            "((lexical_declaration (variable_declarator (identifier) @identifier)))".to_owned(),
        ],
        outline_query: Some(
            r#"
        (class_declaration
          name: (identifier) @definition.class.name
      ) @definition.class
  
      (function_declaration
          name: (identifier) @function.name
          parameters: (formal_parameters
            (identifier) @parameter.identifier
          )? @function.parameters
          body: (statement_block) @function.body
      ) @definition.function
  
      (generator_function_declaration
          name: (identifier) @function.name
          parameters: (formal_parameters
            (identifier) @parameter.identifier
          )? @function.parameters
          body: (statement_block) @function.body
      ) @definition.function
  
      (method_definition
          name: (property_identifier) @function.name
          parameters: (formal_parameters
            (identifier) @parameter.identifier
          )? @function.parameters
          body: (statement_block) @function.body
      ) @definition.method
  
      (arrow_function
        parameters: (formal_parameters
            (identifier) @parameter.identifier
          )? @function.parameters
        body: (statement_block) @function.body
      ) @definition.function
  
      (export_statement
          (function_declaration
              name: (identifier) @function.name
              parameters: (formal_parameters
                (identifier) @parameter.identifier
              )? @function.parameters
              body: (statement_block) @function.body
          )
      ) @definition.function
  
      (export_statement
          (class_declaration
              name: (identifier) @definition.class.name
          )
      ) @definition.class
        "#
            .to_owned(),
        ),
        excluded_file_paths: vec![],
        language_str: "javascript".to_owned(),
        object_qualifier: "(call_expression
          function: (member_expression
            object: (identifier) @path))"
            .to_owned(),
        file_definitions_query: r#"(
          (comment)* @doc
          .
          (method_definition
            name: (property_identifier) @name.definition.method) @definition.method
          (#not-eq? @name.definition.method "constructor")
          (#strip! @doc "^[\\s\\*/]+|^[\\s\\*/]$")
          (#select-adjacent! @doc @definition.method)
        )
        
        (
          (comment)* @doc
          .
          [
            (class
              name: (_) @name.definition.class)
            (class_declaration
              name: (_) @name.definition.class)
          ] @definition.class
          (#strip! @doc "^[\\s\\*/]+|^[\\s\\*/]$")
          (#select-adjacent! @doc @definition.class)
        )
        
        (
          (comment)* @doc
          .
          [
            (function
              name: (identifier) @name.definition.function)
            (function_declaration
              name: (identifier) @name.definition.function)
            (generator_function
              name: (identifier) @name.definition.function)
            (generator_function_declaration
              name: (identifier) @name.definition.function)
          ] @definition.function
          (#strip! @doc "^[\\s\\*/]+|^[\\s\\*/]$")
          (#select-adjacent! @doc @definition.function)
        )
        
        (
          (comment)* @doc
          .
          (lexical_declaration
            (variable_declarator
              name: (identifier) @name.definition.function
              value: [(arrow_function) (function)]) @definition.function)
          (#strip! @doc "^[\\s\\*/]+|^[\\s\\*/]$")
          (#select-adjacent! @doc @definition.function)
        )
        
        (
          (comment)* @doc
          .
          (variable_declaration
            (variable_declarator
              name: (identifier) @name.definition.function
              value: [(arrow_function) (function)]) @definition.function)
          (#strip! @doc "^[\\s\\*/]+|^[\\s\\*/]$")
          (#select-adjacent! @doc @definition.function)
        )
        
        (assignment_expression
          left: [
            (identifier) @name.definition.function
            (member_expression
              property: (property_identifier) @name.definition.function)
          ]
          right: [(arrow_function) (function)]
        ) @definition.function
        
        (pair
          key: (property_identifier) @name.definition.function
          value: [(arrow_function) (function)]) @definition.function
        
        (
          (call_expression
            function: (identifier) @name.reference.call) @reference.call
          (#not-match? @name.reference.call "^(require)$")
        )
        
        (call_expression
          function: (member_expression
            property: (property_identifier) @name.reference.call)
          arguments: (_) @reference.call)
        
        (new_expression
          constructor: (_) @name.reference.class) @reference.class
        
        "#
        .to_owned(),
        required_parameter_types_for_functions: "".to_owned(),
        function_call_path: None,
    }
}
