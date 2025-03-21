use llm_client::clients::types::{LLMClientCompletionRequest, LLMClientMessage};

use crate::agentic::tool::code_edit::types::CodeEdit;

use super::broker::{CodeEditPromptFormatters, CodeSnippetForEditing};

/// How many lines above the selection can we show to the llm
/// this is used to truncate the size of the context we show the llm when its
/// making edits, since we will grab the correct context required almost always
/// with our definitions
const SURROUNDING_CONTEXT_LIMIT: usize = 200;

pub struct AnthropicCodeEditFromatter {}

impl AnthropicCodeEditFromatter {
    pub fn new() -> Self {
        Self {}
    }

    fn system_message_for_code_editing_outline(
        &self,
        language: &str,
        file_path: &str,
        symbol_to_edit: Option<String>,
    ) -> String {
        let symbol_to_edit_instruction = if let Some(symbol_to_edit) = symbol_to_edit {
            format!("- You have to edit the code for {symbol_to_edit} which has been shown to you in <code_to_edit> section.\n")
        } else {
            "".to_owned()
        };
        format!(
            r#"You are an expert software engineer who writes the most high quality code without making any mistakes.
Follow the user's requirements carefully and to the letter.
- The user instructions are present in <user_instruction> tag.
- Modify the code or create new code, the code is present in <code_to_edit>
- The code present above the section you have to edit will be given in <code_above> section.
- The code present below the section you have to edit will be given in <code_below> section.
- The code you have to rewrite will be given to you in <code_to_edit> section.
- Use the additional context provided to you in <extra_data> section to understand the functions available on different types of variables, it might have additional context provided by the user, use them as required.
- There are some additional symbols which we will be creating which you can use right now while editing this section of the code, this is present in <extra_symbols_will_be_created>
- The code you have to edit is in {file_path}
- Output the edited code in a single code block.
- Each code block starts with ```{language}.
- You must always answer in {language} code.
- If you do not have to make any changes to the code, then leave it intact, no need to leave comments explaining that you do not have to make changes. Only use comments or leave comments when you are going to make changes, otherwise do not do it
- Your reply should be contained in the <reply> tags.
- Your reply consists of 2 parts, the first part where you come up with a detailed plan of the changes you are going to do and then the changes. The detailed plan is contained in <thinking> section should not be more than 3 lines and the edited code is present in <code_edited> section.
- Make sure you follow the pattern specified for replying and make no mistakes while doing that.
- You are not allowed to do edits which fall in the following bucket:
- - Renaming: We do not allow renaming since the user has explicitly asked us to never change the name of any of the class, enum, type, method or function.
- - If most of the code is the same, then leave a comment stating that it is the same code (do not write code which is the same)
{symbol_to_edit_instruction}

We are also showing you an example:

<user_instruction>
We want to print the parameters of the function
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
</code_above>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b
</code_below>
<code_to_edit>
```python
    def add(a, b):
        return a + b
</code_to_edit>

Your reply is:
<reply>
<thinking>
The user instruction requires us to print the parameters for the function. I can use the print function in python to do so.
</thinking>
<code_edited>
```python
    def add(a, b):
        print(a, b)
        return a + b
```
</code_edited>
</reply>

We are showing you another example now, where the output is WRONG:
<user_instruction>
We want to print the parameters of the function
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
</code_above>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b
</code_below>
<code_to_edit>
```python
    def add(a, b):
        return a + b
</code_to_edit>

Your reply:
<reply>
<thinking>
The user instruction requires us to print the parameters for the function. I can use the print function in python to do so.
</thinking>
<code_edited>
    def add(a, b):
        print(a, b)
        return a + b

    @class_method
    def multiply(a, b):
        print(a, b)
        return a + b
</code_edited>
</reply>

Notice how you moved beyond the add method and also changed the multiply method which is wrong. We only want to change the add method."#
        )
    }

    fn system_message_for_code_insertion(
        &self,
        language: &str,
        file_path: &str,
        symbol_name: &str,
    ) -> String {
        format!(
            r#"You are an expert software engineer who writes the most high quality code without making any mistakes.
Follow the user's requirements carefully and to the letter.
- The user instructions are present in <user_instruction> tag.
- Create new code for the method {symbol_name} as the user has asked, the code will be put in <code_to_add> section, remember we just want to add the new method.
- The code present above the section where you will be adding code is given in <code_above> section.
- The code present below the section where you will be adding code is given in <code_below> section.
- The code you will be adding is in <code_to_add> section.
- The file content will be shown to you like this:
<code_above>
</code_above>
<code_to_add>
</code_to_add>
<code_below>
</code_below>
We are going to insert the code in the section <code_to_add> of the input, when you write the code for is_grok method we will replace the <code_to_add> section with what you generate.
- Use the additional context provided to you in <extra_data> section to understand the functions available on different types of variables, it might have additional context provided by the user, use them as required.
- There are some additional symbols which we will be creating which you can use right now while editing this section of the code, this is present in <extra_symbols_will_be_created>
- The code you have to edit is in {file_path}
- Output the edited code in a single code block.
- Each code block starts with ```{language}.
- You must always answer in {language} code.
- Your reply should be contained in the <reply> tags.
- Your reply consists of 2 parts, the first part where you come up with a detailed plan of the changes you are going to do and then the changes. The detailed plan is contained in <thinking> section should not be more than 3 lines and the code to add should be written code in <code_to_add> section.
- Make sure you follow the pattern specified for replying and make no mistakes while doing that.
- Make sure to add the new method in <code_to_add> section without leaving any comments or placeholder values.
- The user will use the code which you generated directly without looking at it or taking care of any additional comments, so make sure that the code is complete.

<user_instruction>
Add the function to divide 2 numbers
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
    def add(a, b):
        return a + b
</code_above>
<code_to_add>
{{you need to add code just here}}
</code_to_add>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b

    @class_method
    def power(a, b):
        return pow(a, b)

if __name__ == '__main__':
    # All the operations supported by Maths class
    sum_value = Maths.add(1, 2)
    sub_value = Maths.sub(1, 2)
    mul_value = Maths.multiply(1, 2)
    power = Maths.power(1, 2)
</code_below>

<code_to_add>
</code_to_add>

Your reply is:
<reply>
<thinking>
The user has asked to me to add a function to divide 2 numbers. We have to update the main script as well, but we can not do that now.
</thinking>
<code_edited>
```python
    def divide(a, b):
        return a / b
```
</code_edited>
</reply>

Another bad example for you to avoid:
<user_instruction>
Add the function to divide 2 numbers, we also want to change the main function to use Maths.divide
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
    def add(a, b):
        return a + b
</code_above>
<code_to_add>
{{you need to add code just here}}
</code_to_add>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b

    @class_method
    def power(a, b):
        return pow(a, b)

if __name__ == '__main__':
    # All the operations supported by Maths class
    sum_value = Maths.add(1, 2)
    sub_value = Maths.sub(1, 2)
    mul_value = Maths.multiply(1, 2)
    power = Maths.power(1, 2)
</code_below>

<code_to_add>
</code_to_add>

Your reply is:
<reply>
<thinking>
The user has asked to me to add a function to divide 2 numbers
</thinking>
<code_edited>
```python
    def divide(a, b):
        return a / b

    @class_method
    def multiply(a, b):
        return a * b

    @class_method
    def power(a, b):
        return pow(a, b)

if __name__ == '__main__':
    # All the operations supported by Maths class
    sum_value = Maths.add(1, 2)
    sub_value = Maths.sub(1, 2)
    divide_value = Maths.divide(1, 2)
    mul_value = Maths.multiply(1, 2)
    power = Maths.power(1, 2)
```
</code_edited>
</reply>

In this example the mistake you made is that you went ahead and edited code outside of the <code_to_add> section, we want to always avoid that

- Notice how the indentation is always correct and we are inserting the method at the end of the class, so remember that when presented with the user query and we did not generate any code beyond just the user requirement.
- Do not generate any code which is present in <code_below> only write the code which is in the user requirement and can be inserted in <code_to_add> section
- Never write code which will go beyond the <code_to_add> section"#
        )
    }

    fn few_shot_examples_for_code_editing(
        &self,
        should_disable_thinking: bool,
    ) -> Vec<LLMClientMessage> {
        vec![
            LLMClientMessage::user(
                r#"<user_instruction>
We want to print the parameters of the function
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
</code_above>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b
</code_below>
<code_to_edit>
```python
    def add(a, b):
        return a + b
</code_to_edit>"#
                    .to_owned(),
            ),
            LLMClientMessage::assistant({
                if should_disable_thinking {
                    r#"<reply>
<code_edited>
```python
    def add(a, b):
        print(a, b)
        return a + b
```
</code_edited>
</reply>"#.to_owned()
                } else {
                r#"<reply>
<thinking>
The user instruction requires us to print the parameters for the function. I can use the print function in python to do so.
</thinking>
<code_edited>
```python
    def add(a, b):
        print(a, b)
        return a + b
```
</code_edited>
</reply>"#.to_owned()}}),
            LLMClientMessage::user("<user_instruction>
We want to print the parameters of the function
</user_instruction>

<code_above>
class Maths
    @class_method
    def subtract(a, b):
        return a - b
    
    @class_method
</code_above>
<code_below>
    @class_method
    def multiply(a, b):
        return a * b
</code_below>
<code_to_edit>
```python
    def add(a, b):
        return a + b
</code_to_edit>".to_owned()),
            LLMClientMessage::assistant({
                if should_disable_thinking {
                    r#"<reply>
<code_edited>
    def add(a, b):
        print(a, b)
        return a + b

    @class_method
    def multiply(a, b):
        print(a, b)
        return a + b
</code_edited>
</reply>"#.to_owned()
                } else {
                    r#"<reply>
<thinking>
The user instruction requires us to print the parameters for the function. I can use the print function in python to do so.
</thinking>
<code_edited>
    def add(a, b):
        print(a, b)
        return a + b

    @class_method
    def multiply(a, b):
        print(a, b)
        return a + b
</code_edited>
</reply>"#.to_owned()}}),
            LLMClientMessage::user("you moved beyond the add method and also changed the multiply method which is wrong. We only want to change the add method.".to_owned()),
            LLMClientMessage::assistant("Understood I will only edit code in the section which has been mentioned by the user and never go beyond it".to_owned())
        ]
    }

    fn system_message(&self, disable_thinking: bool) -> String {
        let should_thinking_present = if !disable_thinking {
            "- Your reply consists of 2 parts, the first part where you come up with a detailed plan of the changes you are going to do and then the changes. The detailed plan is contained in <thinking> section and the edited code is present in <code_edited> section.".to_owned()
        } else {
            "- Your reply should consist of a single section called <code_edited> where you edit the code based on user instruction.".to_owned()
        };
        format!(
            r#"You are an expert software engineer who writes the most high quality code without making any mistakes.
Follow the user's requirements carefully and to the letter.
- The user instructions are present in <user_instruction> tag.
- Modify the code or create new code, the code is present in <code_to_edit>
- The code present above the section you have to edit will be given in <code_above> section.
- The code present below the section you have to edit will be given in <code_below> section.
- The code you have to rewrite will be given to you in <code_to_edit> section.
- Use the additional context provided to you in <extra_data> section to understand the functions available on different types of variables, it might have additional context provided by the user, use them as required.
- There are some additional symbols which we will be creating which you can use right now while editing this section of the code, this is present in <extra_symbols_will_be_created>
- The user has provided you additional code to follow same patterns from and understand how the code is supposed to be written, this is provided in <user_provided_context_files>.
- <user_provided_context_files> might also include an older version of the code which you have to edit, so do not fixtate on that and follow the user instructions.
- Output the edited code in a single code block.
- Each code block starts with ``` and the coding language for the selected file.
- You must always answer in code block.
- Your reply should be contained in the <reply> tags.
{should_thinking_present}
- Make sure you follow the pattern specified for replying and make no mistakes while doing that.
- Make sure to rewrite the whole code present in <code_to_edit> without leaving any comments or using place-holders.
- The user will use the code which you generated directly without looking at it or taking care of any additional comments, so make sure that the code is complete."#
        )
    }

    fn user_message_for_code_editing(&self, context: &CodeEdit) -> Vec<LLMClientMessage> {
        let mut messages = vec![];
        if let Some(user_provided_context) = context.user_provided_context() {
            let context_message = format!(
                r#"<user_provided_context_files>
{user_provided_context}
</user_provided_context_files>"#
            );
            if context_message
                .lines()
                .into_iter()
                .collect::<Vec<_>>()
                .len()
                > 50
            {
                messages.push(LLMClientMessage::user(context_message.to_owned()).cache_point());
            }
        }
        let extra_data = self.extra_data(context.extra_content());
        let above = self.above_selection(
            context
                .above_context()
                .map(|code_above| {
                    // limit it to 100 lines from the start
                    let mut lines = code_above.lines().collect::<Vec<_>>();
                    lines.reverse();
                    lines.truncate(SURROUNDING_CONTEXT_LIMIT);
                    lines.reverse();
                    lines.join("\n")
                })
                .as_deref(),
        );
        let below = self.below_selection(
            context
                .below_context()
                .map(|code_below| {
                    let mut lines = code_below.lines().collect::<Vec<_>>();
                    lines.truncate(SURROUNDING_CONTEXT_LIMIT / 3);
                    lines.join("\n")
                })
                .as_deref(),
        );
        let in_range = self.selection_to_edit(context.code_to_edit());
        let mut user_message = "".to_owned();
        let extra_symbols_to_be_created = context.symbols_which_will_be_added();
        if let Some(extra_symbols_to_be_created) = extra_symbols_to_be_created {
            user_message = user_message
                + &format!(
                    r#"<extra_symbols_will_be_created>
{extra_symbols_to_be_created}
</extra_symbols_will_be_created>"#
                )
                + "\n";
        }
        user_message = user_message + &extra_data + "\n";
        if let Some(above) = above {
            user_message = user_message + &above + "\n";
        }
        if let Some(below) = below {
            user_message = user_message + &below + "\n";
        }
        user_message = user_message + &in_range + "\n";

        // Now we add the instruction from the user
        let user_instruction = context.instruction();
        user_message = user_message
            + &format!(
                r#"Only edit the code in <code_to_edit> section, my instructions are:
<user_instruction>
{user_instruction}
</user_instruction>"#
            );
        messages.push(LLMClientMessage::user(user_message));
        messages
    }

    fn user_message_for_code_addition(
        &self,
        context: &CodeEdit,
        new_sub_symbol: String,
    ) -> Vec<LLMClientMessage> {
        let extra_data = self.extra_data(context.extra_content());
        let above = self.above_selection(context.above_context());
        // let below = self.below_selection(context.below_context());
        let mut user_message = "".to_owned();
        let extra_symbols_to_be_created = context.symbols_which_will_be_added();
        if let Some(extra_symbols_to_be_created) = extra_symbols_to_be_created {
            user_message = user_message
                + &format!(
                    r#"<extra_symbols_will_be_created>
{extra_symbols_to_be_created}
</extra_symbols_will_be_created>"#
                )
                + "\n";
        }
        user_message = user_message + &extra_data + "\n";
        if let Some(above) = above {
            user_message = user_message + &above + "\n";
        }
        let in_range = context.code_to_edit();
        user_message = user_message
            + &format!(
                r#"<code_to_add>
{{you need to add code for {new_sub_symbol} just here}}
</code_to_add>
{in_range}
"#
            );
        // if let Some(below) = below {
        //     user_message = user_message + &below + "\n";
        // }

        let user_instructions = context.instruction();
        user_message = user_message
            + &format!(
                r#"Only add the code in <code_to_add> section, my instructions are:
<user_instructions>
{user_instructions}
</user_instructions>"#
            );
        vec![LLMClientMessage::user(user_message)]
    }

    fn above_selection(&self, above_selection: Option<&str>) -> Option<String> {
        if let Some(above_selection) = above_selection {
            Some(format!(
                r#"I have the following code above:
<code_above>
{above_selection}
</code_above>"#
            ))
        } else {
            None
        }
    }

    fn below_selection(&self, below_selection: Option<&str>) -> Option<String> {
        if let Some(below_selection) = below_selection {
            Some(format!(
                r#"I have the following code below:
<code_below>
{below_selection}
</code_below>"#
            ))
        } else {
            None
        }
    }

    fn selection_to_edit(&self, selection_to_edit: &str) -> String {
        format!(
            r#"I have the following code in selection to edit:
<code_to_edit>
{selection_to_edit}
</code_to_edit>"#
        )
    }

    fn extra_data(&self, extra_data: &str) -> String {
        format!(
            r#"This is the extra data which you can use:
<extra_data>
{extra_data}
</extra_data>"#
        )
    }

    fn system_message_for_code_to_edit(&self) -> String {
        format!("You are an expert software engineer tasked with finding the right code snippets where edits need to be made for satisfying the user request.
You will be given user instructions in the <user_instruction> section, and the file along with the contents in <file> section.
The file has been divided into sections like so:
<file>
<path>some_file_path</path>
<section>
<id>1</id>
<content>
file_content...
</content>
<id>2</id>
<content>
file_content...
</content>
.... more contents
</file>

You have to first think step by step on how the change can be done, and then select the sections of the file where the changes need to be done along with your reasoning.

As an example:
<file>
<path>tests/calculate.py</path>
<section>
<id>1</id>
<content>
import unittest
from calculator import Calculator

class TestAddition(unittest.TestCase):
    def setUp(self):
        self.calc = Calculator()

    def test_add_positive_numbers(self):
        result = self.calc.add(2, 3)
        self.assertEqual(result, 5)

    def test_add_negative_numbers(self):
        result = self.calc.add(-2, -3)
        self.assertEqual(result, -5)

    def test_add_zero(self):
        result = self.calc.add(0, 0)
        self.assertEqual(result, 0)

</content>
</section>
<section>
<id>
2
</id>
<content>
class TestSubtraction(unittest.TestCase):
    def setUp(self):
        self.calc = Calculator()

    def test_subtract_positive_numbers(self):
        result = self.calc.subtract(5, 3)
        self.assertEqual(result, 2)

    def test_subtract_negative_numbers(self):
        result = self.calc.subtract(-5, -3)
        self.assertEqual(result, -2)

    def test_subtract_zero(self):
        result = self.calc.subtract(5, 0)
        self.assertEqual(result, 5)

</content>
</section>
<section>
<id>
3
</id>
<content>
class TestMultiplication(unittest.TestCase):
    def setUp(self):
        self.calc = Calculator()

    def test_multiply_positive_numbers(self):
        result = self.calc.multiply(2, 3)
        self.assertEqual(result, 6)

    def test_multiply_negative_numbers(self):
        result = self.calc.multiply(-2, 3)
        self.assertEqual(result, -6)

    def test_multiply_by_zero(self):
        result = self.calc.multiply(5, 0)
        self.assertEqual(result, 0)

</content>
</section>
</file>

<user_instruction>
We are modifying the test case for multiplying 2 positive numbers in the calculator_test.py file.
</user_instruction>

Your reply should be the in the following format:
<reply>
<sections>
<section>
<id>
3
</id>
<thinking>
We need to select this block to edit because this is where the test for multiplying 2 positive numbers is present. 
</thinking>
</section>
</sections>
</reply>
")
    }
}

impl CodeEditPromptFormatters for AnthropicCodeEditFromatter {
    fn format_prompt(&self, context: &CodeEdit) -> LLMClientCompletionRequest {
        let language = context.language();
        let should_disable_thinking = context.disable_thinking();
        let fs_file_path = context.fs_file_path();
        let system_message = if context.is_outline_edit() {
            // new sub-symbol implies we are adding code, the system prompt
            // looks a bit different for that case
            if let Some(new_sub_symbol) = context.is_new_sub_symbol() {
                self.system_message_for_code_insertion(language, fs_file_path, &new_sub_symbol)
            } else {
                self.system_message_for_code_editing_outline(
                    language,
                    fs_file_path,
                    context.symbol_to_edit_name(),
                )
            }
        } else {
            if let Some(new_sub_symbol) = context.is_new_sub_symbol() {
                self.system_message_for_code_insertion(language, fs_file_path, &new_sub_symbol)
            } else {
                self.system_message(should_disable_thinking)
            }
        };
        let few_shot_examples = if context.is_new_sub_symbol().is_some() {
            vec![]
        } else {
            self.few_shot_examples_for_code_editing(should_disable_thinking)
        };
        let user_messages = if let Some(sub_symbol_name) = context.is_new_sub_symbol() {
            self.user_message_for_code_addition(context, sub_symbol_name)
        } else {
            self.user_message_for_code_editing(context)
        };

        let mut messages = vec![];

        // add the system message
        messages.push(LLMClientMessage::system(system_message));
        messages.extend(few_shot_examples);
        messages.extend(user_messages);

        // we use 0.2 temperature so the model can imagine ✨
        LLMClientCompletionRequest::new(context.model().clone(), messages, 0.2, None)
    }

    fn find_code_section(&self, context: &CodeSnippetForEditing) -> LLMClientCompletionRequest {
        // we might want to either add new code or find the code to edit
        // code to edit might be pretty simple, since we can figure out what needs to be done
        // code to add is tricky because we want to find the code location where we want to place it
        // are we going to send symbols or are we going to send whole code blocks?
        // we can also look at the recently edited line in this file which might get a priority over here
        // we can show that with a + mark across the line for each of use and figuring out
        // how to make changes (excluding the imports which we will fix later on)
        let snippets = context.snippets();
        let file_path = context.file_path();
        let user_instruction = context.user_query();
        let formatted_snippets = snippets
            .into_iter()
            .enumerate()
            .map(|(idx, snippet)| {
                let content = snippet.snippet_content();
                format!(
                    r#"<section>
<id>
{idx}
</id>
<content>
{content}
</content>
</section>"#
                )
                .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let user_message = format!(
            r#"<file>
<path>{file_path}</path>
{formatted_snippets}
</file>

<user_instruction>
{user_instruction}
</user_instruction>"#
        );

        let system_message = self.system_message_for_code_to_edit();
        LLMClientCompletionRequest::new(
            context.model().clone(),
            vec![
                LLMClientMessage::system(system_message),
                LLMClientMessage::user(user_message),
            ],
            0.2,
            None,
        )
    }
}
