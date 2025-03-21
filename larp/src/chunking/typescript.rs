use crate::chunking::languages::TSLanguageConfig;

pub fn typescript_language_config() -> TSLanguageConfig {
    TSLanguageConfig {
        language_ids: &["Typescript", "TSX", "typescript", "tsx"],
        file_extensions: &["ts", "tsx", "jsx", "mjs"],
        grammar: tree_sitter_typescript::language_tsx,
        namespaces: vec![vec![
            "constant",
            "variable",
            "property",
            "parameter",
            // functions
            "function",
            "method",
            "generator",
            // types
            "alias",
            "enum",
            "enumerator",
            "class",
            "interface",
            // misc.
            "label",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect()],
        documentation_query: vec!["((comment) @comment
        (#match? @comment \"^\\\\/\\\\*\\\\*\")) @docComment"
            .to_owned(), "(comment) @comment".to_owned()],
        function_query: vec!["[
            (function_declaration
                name: (identifier)? @identifier
                parameters: (formal_parameters
                    (required_parameter
                        pattern: (identifier) @parameter.identifier) 
                )? @parameters
                return_type: (type_annotation)? @return_type
                body: (statement_block
                    (lexical_declaration
                      (variable_declarator
                        name: (identifier) @variable.name
                        type: (type_annotation)? @variable.type
                      )
                    )*
                  )? @body)
            (generator_function
                name: (identifier)? @identifier
                parameters: (formal_parameters
                    (required_parameter
                        pattern: (identifier) @parameter.identifier) 
                )? @parameters
                return_type: (type_annotation)? @return_type
                body: (statement_block
                    (lexical_declaration
                      (variable_declarator
                        name: (identifier) @variable.name
                        type: (type_annotation)? @variable.type
                      )
                    )*
                  )? @body)
            (generator_function_declaration
                name: (identifier)? @identifier
                parameters: (formal_parameters
                    (required_parameter
                        pattern: (identifier) @parameter.identifier) 
                )? @parameters
                return_type: (type_annotation)? @return_type
                body: (statement_block
                    (lexical_declaration
                      (variable_declarator
                        name: (identifier) @variable.name
                        type: (type_annotation)? @variable.type
                      )
                    )*
                  )? @body)
            (method_definition
                name: (property_identifier)? @identifier
                parameters: (formal_parameters
                    (required_parameter
                        pattern: (identifier) @parameter.identifier) 
                )? @parameters
                return_type: (type_annotation)? @return_type
                body: (statement_block
                    (lexical_declaration
                      (variable_declarator
                        name: (identifier) @variable.name
                        type: (type_annotation)? @variable.type
                      )
                    )*
                  )? @body)
            (arrow_function
                body: (statement_block
                    (lexical_declaration
                      (variable_declarator
                        name: (identifier) @variable.name
                        type: (type_annotation)? @variable.type
                      )
                    )*
                  )? @body
                  parameters: (formal_parameters
                    (required_parameter
                        pattern: (identifier) @parameter.identifier) 
                )? @parameters
                return_type: (type_annotation)? @return_type)
            ] @function"
            .to_owned()],
        construct_types: vec![
            "program",
            "interface_declaration",
            "class_declaration",
            "function_declaration",
            "function",
            "type_alias_declaration",
            "method_definition",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        expression_statements: vec![
            "lexical_declaration",
            "expression_statement",
            "public_field_definition",
        ]
        .into_iter()
        .map(|s| s.to_owned())
        .collect(),
        class_query: vec![
            "[(abstract_class_declaration name: (type_identifier)? @identifier) (class_declaration name: (type_identifier)? @identifier)] @class_declaration"
                .to_owned(),
        ],
        r#type_query: vec![
            "[(type_alias_declaration name: (type_identifier) @identifier)] @type_declaration"
                .to_owned(),
        ],
        namespace_types: vec![
            "export_statement".to_owned(),
        ],
        hoverable_query: r#"
        [(identifier)
         (property_identifier)
         (shorthand_property_identifier)
         (shorthand_property_identifier_pattern)
         (statement_identifier)
         (type_identifier)] @hoverable
        "#.to_owned(),
        comment_prefix: "//".to_owned(),
        end_of_line: Some(";".to_owned()),
        // TODO(skcd): Add missing cases here as required
        import_identifier_queries: r#"
(
    import_statement
        (import_clause
            (named_imports
                (import_specifier
                    name: (identifier) @import_identifier
                )
            )
        )
)
(
    import_statement
        (import_clause
            (namespace_import
                (identifier) @import_identifer
            )
        )
)
        "#.to_owned(),
        block_start: Some("{".to_owned()),
        variable_identifier_queries: vec![
            "((lexical_declaration (variable_declarator (identifier) @identifier)))"
                .to_owned(),
        ],
        outline_query: Some(r#"
        (class_declaration
          name: (type_identifier) @definition.class.name
      ) @definition.class
      
      (abstract_class_declaration
        name: (type_identifier)? @definition.class.name
      ) @definition.class
      
      (enum_declaration
        name: (identifier)? @definition.class.name
      ) @definition.class
  
      (interface_declaration
          name: (type_identifier) @definition.class.name
      ) @definition.class.declaration
  
      (type_alias_declaration
          name: (type_identifier) @definition.class.name
      ) @definition.class.declaration
  
      (method_definition
          name: (property_identifier) @function.name
          body: (statement_block) @function.body
      ) @definition.method
  
      (function_declaration
          name: (identifier) @function.name
          body: (statement_block) @function.body
      ) @definition.function
  
      (export_statement
          (function_declaration
              name: (identifier) @function.name
              body: (statement_block) @function.body
          )
      ) @definition.function
  
      (export_statement
          (class_declaration
              name: (type_identifier) @definition.class.name
          )
      ) @definition.class
  
      (export_statement
          (interface_declaration
              name: (type_identifier) @definition.class.name
          )
      ) @definition.class.declaration
  
      (export_statement
          (type_alias_declaration
              name: (type_identifier) @definition.class.name
          )
      ) @definition.class.declaration
        "#.to_owned()),
        excluded_file_paths: vec![],
        language_str: "typescript".to_owned(),
        object_qualifier: r#"(call_expression
                function: (member_expression
                  object: (identifier) @path))"#
                .to_owned(),
        file_definitions_query: r#"(function_signature
            name: (identifier) @name.definition.function) @definition.function
          
          (method_signature
            name: (property_identifier) @name.definition.method) @definition.method
          
          (abstract_method_signature
            name: (property_identifier) @name.definition.method) @definition.method
          
          (abstract_class_declaration
            name: (type_identifier) @name.definition.class) @definition.class
          
          (module
            name: (identifier) @name.definition.module) @definition.module
          
          (interface_declaration
            name: (type_identifier) @name.definition.interface) @definition.interface
          
          (type_annotation
            (type_identifier) @name.reference.type) @reference.type
          
          (new_expression
            constructor: (identifier) @name.reference.class) @reference.class
          
          (function_declaration
            name: (identifier) @name.definition.function) @definition.function
          
          (method_definition
            name: (property_identifier) @name.definition.method) @definition.method
          
          (class_declaration
            name: (type_identifier) @name.definition.class) @definition.class
          
          (interface_declaration
            name: (type_identifier) @name.definition.class) @definition.class
          
          (type_alias_declaration
            name: (type_identifier) @name.definition.type) @definition.type
          
          (enum_declaration
            name: (identifier) @name.definition.enum) @definition.enum
        "#.to_owned(),
        required_parameter_types_for_functions: r#"
(required_parameter
  type: (type_annotation) @type_annotation?
)
        "#.to_owned(),
        function_call_path: None,
    }
}
