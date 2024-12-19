//! We want to invoke the code edit and rewrite a section of the code which we
//! are insterested in
//! The input here is the file_path and the range to edit and the new_output which
//! we want to generate

use std::sync::Arc;

use async_trait::async_trait;
use llm_client::{broker::LLMBroker, clients::types::LLMType};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    agentic::{
        symbol::{
            identifier::{LLMProperties, SymbolIdentifier},
            ui_event::UIEventWithID,
        },
        tool::{
            errors::ToolError,
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    chunking::text_document::Range,
};

use super::models::broker::CodeEditBroker;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CodeEditingPartialRequest {
    fs_file_path: String,
    instruction: String,
}

impl CodeEditingPartialRequest {
    pub fn new(fs_file_path: String, instruction: String) -> Self {
        Self {
            fs_file_path,
            instruction,
        }
    }
    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn instruction(&self) -> &str {
        &self.instruction
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<thinking>
...
</thinking>
<code_edit_input>
<fs_file_path>
{}
</fs_file_path>
<instruction>
{}
</instruction>
</code_edit_input>"#,
            self.fs_file_path, self.instruction
        )
    }
}

#[derive(Clone, Debug)]
pub struct CodeEdit {
    code_above: Option<String>,
    code_below: Option<String>,
    fs_file_path: String,
    code_to_edit: String,
    extra_context: String,
    language: String,
    instruction: String,
    llm_properties: LLMProperties,
    is_swe_bench_initial_edit: bool,
    symbol_to_edit: Option<String>,
    is_new_symbol_request: Option<String>,
    root_request_id: String,
    edit_range: Range,
    // If this edit is just generating an outline of the changes which need to happen
    // in the symbol and not the complete change which needs to happen
    is_outline_edit: bool,
    new_symbols: Option<String>,
    should_stream: bool,
    symbol_identifier: SymbolIdentifier,
    ui_sender: UnboundedSender<UIEventWithID>,
    // if we should disable thinking and just generate code edits as required
    // this improves the time to first edit
    disable_thinking: bool,
    // This is the context which the user has provided, the hope is that this will
    // be cached and passed along so we do not have to worry about the inference
    // speed on this
    user_provided_context: Option<String>,
    // The session id to which this edit belongs
    session_id: String,
    // The exchange id to which the edit belongs
    exchange_id: String,
}

impl CodeEdit {
    pub fn new(
        code_above: Option<String>,
        code_below: Option<String>,
        fs_file_path: String,
        code_to_edit: String,
        extra_context: String,
        language: String,
        instruction: String,
        llm_properties: LLMProperties,
        is_swe_bench_initial_edit: bool,
        symbol_to_edit: Option<String>,
        is_new_symbol_request: Option<String>,
        root_request_id: String,
        edit_range: Range,
        is_outline_edit: bool,
        new_symbols: Option<String>,
        should_stream: bool,
        symbol_identifier: SymbolIdentifier,
        ui_sender: UnboundedSender<UIEventWithID>,
        disable_thinking: bool,
        user_provided_context: Option<String>,
        session_id: String,
        exchange_id: String,
    ) -> Self {
        Self {
            code_above,
            code_below,
            fs_file_path,
            code_to_edit,
            extra_context,
            language,
            llm_properties,
            instruction,
            is_swe_bench_initial_edit,
            symbol_to_edit,
            is_new_symbol_request,
            root_request_id,
            edit_range,
            is_outline_edit,
            new_symbols,
            should_stream,
            symbol_identifier,
            ui_sender,
            disable_thinking,
            user_provided_context,
            session_id,
            exchange_id,
        }
    }
}

pub struct CodeEditingTool {
    llm_client: Arc<LLMBroker>,
    broker: Arc<CodeEditBroker>,
    editor_config: Option<LLMProperties>,
    fail_over_llm: LLMProperties,
}

/// `CodeEditingTool` is responsible for handling code editing operations.
/// It manages the interaction with language models for code editing tasks,
/// including handling retries, formatting prompts, and processing responses.
impl CodeEditingTool {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        broker: Arc<CodeEditBroker>,
        fail_over_llm: LLMProperties,
    ) -> Self {
        Self {
            llm_client,
            broker,
            editor_config: None,
            fail_over_llm,
        }
    }

    pub fn set_editor_config(mut self, editor_config: Option<LLMProperties>) -> Self {
        self.editor_config = editor_config;
        self
    }

    pub fn get_llm_properties(&self) -> Option<&LLMProperties> {
        self.editor_config.as_ref()
    }

    /// Code output from LLMs is of the following form:
    /// {garbage}
    /// <reply>
    /// <thinking>
    /// thinking inside....
    /// </thinking>
    /// <code_edited>
    /// ```{language}
    /// {content}
    /// ```
    /// </code_edited>
    /// </reply>
    /// {garbage}
    /// So we find this pattern and trim it out if we can
    fn edit_code(
        code: &str,
        new_sub_symbol: bool,
        section_to_edit: &str,
    ) -> Result<String, ToolError> {
        let tag_to_search = if new_sub_symbol {
            "code_to_add"
        } else {
            "code_edited"
        };
        let lines = code
            .lines()
            .skip_while(|line| !line.contains(&format!("<{tag_to_search}>")))
            .skip(1)
            .take_while(|line| !line.contains(&format!("</{tag_to_search}>")))
            .collect::<Vec<_>>()
            .into_iter()
            .skip_while(|line| !line.contains("```"))
            .skip(1)
            .take_while(|line| !line.contains("```"))
            .collect::<Vec<_>>()
            .join("\n");
        if lines == "" {
            Err(ToolError::CodeNotFormatted(code.to_owned()))
        } else {
            if new_sub_symbol {
                Ok(lines + "\n" + section_to_edit + "\n")
            } else {
                Ok(lines)
            }
        }
    }
}

impl CodeEdit {
    pub fn instruction(&self) -> &str {
        &self.instruction
    }

    pub fn above_context(&self) -> Option<&str> {
        self.code_above
            .as_ref()
            .map(|above_context| above_context.as_str())
    }

    pub fn below_context(&self) -> Option<&str> {
        self.code_below
            .as_ref()
            .map(|below_context| below_context.as_str())
    }

    pub fn code_to_edit(&self) -> &str {
        &self.code_to_edit
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn extra_content(&self) -> &str {
        &self.extra_context
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn model(&self) -> &LLMType {
        self.llm_properties.llm()
    }

    pub fn is_new_sub_symbol(&self) -> Option<String> {
        self.is_new_symbol_request.clone()
    }

    pub fn symbol_to_edit_name(&self) -> Option<String> {
        self.symbol_to_edit.clone()
    }

    /// Returns if this is an outline edit and not a deep verbose edit which
    /// we want to perform
    pub fn is_outline_edit(&self) -> bool {
        self.is_outline_edit
    }

    /// Contains the xml string which has symbols which will be edited
    pub fn symbols_which_will_be_added(&self) -> Option<String> {
        self.new_symbols.clone()
    }

    /// If we should disable thinking over here
    pub fn disable_thinking(&self) -> bool {
        self.disable_thinking
    }

    /// returns the user provided context which is supposed to be cached always
    pub fn user_provided_context(&self) -> Option<String> {
        self.user_provided_context.clone()
    }
}

#[async_trait]
impl Tool for CodeEditingTool {
    // TODO(skcd): Figure out how we want to do streaming here in the future
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let code_edit_context = input.is_code_edit()?;
        let root_id = code_edit_context.root_request_id.to_owned();
        let exchange_id = code_edit_context.exchange_id.to_owned();
        let should_stream = code_edit_context.should_stream;
        let ui_sender = code_edit_context.ui_sender.clone();
        let symbol_identifier = code_edit_context.symbol_identifier.clone();
        let selection_range = code_edit_context.edit_range.clone();
        let session_id = code_edit_context.session_id.to_owned();
        let fs_file_path = code_edit_context.fs_file_path.to_owned();
        let mut llm_message = self.broker.format_prompt(&code_edit_context)?;
        if let Some(llm_properties) = self.get_llm_properties() {
            llm_message = llm_message.set_llm(llm_properties.llm().clone());
        }
        // If this is not special swe bench initial edit then do the overrideas
        // as before
        let (request_llm, request_api_key, request_provider) =
            if !code_edit_context.is_swe_bench_initial_edit {
                if let Some(llm_properties) = self.get_llm_properties() {
                    (
                        llm_properties.llm().clone(),
                        llm_properties.api_key().clone(),
                        llm_properties.provider().clone(),
                    )
                } else {
                    (
                        code_edit_context.llm_properties.llm().clone(),
                        code_edit_context.llm_properties.api_key().clone(),
                        code_edit_context.llm_properties.provider().clone(),
                    )
                }
            // if this is the special swe bench initial edit, then keep the llm properties
            // as they are being sent from the invoker
            } else {
                (
                    code_edit_context.llm_properties.llm().clone(),
                    code_edit_context.llm_properties.api_key().clone(),
                    code_edit_context.llm_properties.provider().clone(),
                )
            };
        llm_message = llm_message.set_llm(request_llm.clone());
        let mut retries = 0;
        // if we are not streaming we get more tries over here
        let retry_limit = if should_stream { 1 } else { 4 };
        loop {
            if retries >= retry_limit {
                return Err(ToolError::RetriesExhausted);
            }
            let (llm, api_key, provider) = if retries % 2 == 0 {
                (
                    request_llm.clone(),
                    request_api_key.clone(),
                    request_provider.clone(),
                )
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };
            let cloned_llm_message = llm_message.clone().set_llm(llm);

            let stream_result;
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut llm_response = Box::pin(
                self.llm_client.stream_completion(
                    api_key,
                    cloned_llm_message,
                    provider,
                    vec![
                        ("event_type".to_owned(), "code_edit_tool".to_owned()),
                        ("root_id".to_owned(), root_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                ),
            );

            let (edits_sender, mut edits_receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut answer_accumulator = CodeToAddAccumulator::new(edits_sender);
            let edit_request_id = uuid::Uuid::new_v4().to_string();

            loop {
                tokio::select! {
                    stream_msg = receiver.recv() => {
                        match stream_msg {
                            Some(msg) => {
                                let delta = msg.delta();
                                if let Some(delta) = delta {
                                    answer_accumulator.add_delta(delta.to_owned());
                                }
                            }
                            None => {
                                // we are probably done over here and the channel is closed?
                            }
                        }
                    }
                    edits_response = edits_receiver.recv() => {
                        if should_stream {
                            match edits_response {
                                Some(CodeBlockEditDelta::EditStarted) => {
                                    let _ = ui_sender.send(UIEventWithID::start_edit_streaming(
                                        root_id.to_owned(),
                                        symbol_identifier.clone(),
                                        edit_request_id.to_owned(),
                                        selection_range,
                                        fs_file_path.to_owned(),
                                        session_id.to_owned(),
                                        exchange_id.to_owned(),
                                        None,
                                    ));
                                }
                                Some(CodeBlockEditDelta::EditDelta(delta)) => {
                                    let _ = ui_sender.send(UIEventWithID::delta_edit_streaming(
                                        root_id.to_owned(),
                                        symbol_identifier.clone(),
                                        delta,
                                        edit_request_id.to_owned(),
                                        selection_range,
                                        fs_file_path.to_owned(),
                                        session_id.to_owned(),
                                        exchange_id.to_owned(),
                                        None,
                                    ));
                                }
                                Some(CodeBlockEditDelta::EditEnd) => {
                                    let _ = ui_sender.send(UIEventWithID::end_edit_streaming(
                                        root_id.to_owned(),
                                        symbol_identifier.clone(),
                                        edit_request_id.to_owned(),
                                        selection_range,
                                        fs_file_path.to_owned(),
                                        session_id.to_owned(),
                                        exchange_id.to_owned(),
                                        None,
                                    ));
                                }
                                None => {

                                }
                            }
                        }
                    }
                    result = &mut llm_response => {
                        stream_result = Some(result);
                        break;
                    }
                }
            }
            match stream_result {
                Some(Ok(response)) => {
                    let edited_code = Self::edit_code(
                        &response,
                        code_edit_context.is_new_sub_symbol().is_some(),
                        code_edit_context.code_to_edit(),
                    )
                    // we need to do post-processing here to remove all the gunk
                    // which usually gets added when we are editing code
                    .map(|result| ToolOutput::code_edit_output(result));
                    match edited_code {
                        Ok(response) => return Ok(response),
                        Err(_e) => {
                            retries = retries + 1;
                            continue;
                        }
                    }
                }
                _ => {
                    retries = retries + 1;
                    continue;
                }
            }
        }
    }

    fn tool_description(&self) -> String {
        "### code_edit_input
Edit a file. The tool is able to edit the file precisely based on instruction. If the file doesn't exist, it will be CREATED. The tool will automatically CREATE any directories needed to write the file.".to_owned()
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters: 
- fs_file_path: (required) The ABSOLUTE path of the file to write to, will be created if not already present
- instruction: (required) The edit instruction, if you are going to output code blocks make sure they are properly placed in ```{{language}} blocks so we can present them properly to the user. Only given instructions here which are concise and contain the relevant changes, do not be verbose.

Usage:
<code_edit_input>
<fs_file_path>
File path here
</fs_file_path>
<instruction>
Edit instruction here
</instruction>
</code_edit_input>

The fs_file_path here needs to be the ABSOLUTE and never the relative path."#
        )
    }

    fn get_evaluation_criteria(&self, trajectory_length: usize) -> Vec<String> {
        let mut evaluation_criteria = if trajectory_length < 3 {
            vec![
                "Exploratory Actions: Recognize that initial searches and information-gathering steps are essential and should not be heavily penalized if they don't yield immediate results.",
                "Appropriateness of Action: Evaluate if the action is logical given the agent's current knowledge and the early stage of problem-solving.",
            ]
        } else {
            vec![
                "Solution Quality: Assess the logical changes, contextual fit, and overall improvement without introducing new issues.",
                "Progress Assessment: Evaluate the agent's awareness of solution history, detection of repetitive actions, and planned next steps.",
                "Repetitive or Redundant Actions: Detect if the agent is repeating the same unsuccessful or redundant actions without making progress. Pay close attention to the agent's history and outputs indicating lack of progress.",
            ]
        };
        evaluation_criteria.extend(vec![
            "Instruction Clarity: Ensure that instructions and pseudocode are clear and actionable.",
            "Instruction Compliance: The git diff must *exactly* implement the provided pseudo_code. Identify any discrepancies, omissions, or additions. If discrepancies exist, you should lower the reward accordingly.",
            "Code Modification Accuracy and Quality: Check for correct identification of code spans, accuracy of changes, syntax errors, logical flaws, unintended modifications, and unintended side effects.",
            "Python-Specific Features Utilization: Assess whether the agent has appropriately utilized Python-specific features that enhance the solution.",
            "Common Git Diff Issues and Unintended Changes: Check for issues such as incorrect line numbers, unintended additions or deletions, formatting errors, changes to unrelated parts of the code, and heavily penalize unintended changes.",
            "Addressing Test Failures: Verify if the agent is properly addressing test failures from previous `RunTests` actions.",
        ]);
        evaluation_criteria
            .into_iter()
            .map(|evaluation_criteria| evaluation_criteria.to_owned())
            .collect()
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new(
                90,
                100,
                "The code change is optimal, with a perfect Git diff exactly matching the pseudo code, and requires no further changes.",
            ),
            ToolRewardScale::new(
                75,
                89,
                "The code change significantly advances the solution, with an accurate Git diff exactly matching the pseudo code,.",
            ),
            ToolRewardScale::new(
                50,
                74,
                "The code change is mostly correct but has minor issues or opportunities for optimization; the Git diff exactly matching the pseudo code,.",
            ),
            ToolRewardScale::new(
                25,
                49,
                "The code change is acceptable but has noticeable issues or is less effective than possible alternatives;",
            ),
            ToolRewardScale::new(
                0,
                24,
                "The code change has minimal impact or introduces minor negative consequences",
            ),
            ToolRewardScale::new(
                -49,
                -1,
                "The code change is inappropriate, unhelpful, or introduces new issues; the action did not result in any successful code changes. The Git diff does not match the pseud code and instructions, contains significant inaccuracies or shows no changes. Penalize attempts to modify non-existent code elements (hallucinations) based on severity.",
            ),
            ToolRewardScale::new(
                -100,
                -50,
                "The code change is counterproductive, causing significant setbacks or demonstrating persistent repetition without learning. The Git diff is severely flawed or indicates that no effective changes were made. Heavily penalize severe hallucinations or continuous attempts to modify non-existent code elements.",
            ),
        ]
    }
}

enum CodeBlockEditDelta {
    EditStarted,
    EditDelta(String),
    EditEnd,
}

#[derive(Debug, Clone)]
enum CodeToAddBlockStatus {
    NoBlock,
    BlockStart,
    BlockAccumualtor(String),
}

struct CodeToAddAccumulator {
    answer_up_until_now: String,
    previous_answer_line_number: Option<usize>,
    code_to_add_block_status: CodeToAddBlockStatus,
    sender: UnboundedSender<CodeBlockEditDelta>,
}

impl CodeToAddAccumulator {
    pub fn new(sender: UnboundedSender<CodeBlockEditDelta>) -> Self {
        Self {
            answer_up_until_now: "".to_owned(),
            previous_answer_line_number: None,
            code_to_add_block_status: CodeToAddBlockStatus::NoBlock,
            sender,
        }
    }

    pub fn _answer_up_until_now(&self) -> String {
        self.answer_up_until_now.to_owned()
    }

    fn add_delta(&mut self, delta: String) {
        self.answer_up_until_now.push_str(&delta);
        self.process_answer();
    }

    fn process_answer(&mut self) {
        let head = vec!["<code_to_add>", "<code_edited>"];
        let tail = vec!["</code_to_add>", "</code_edited>"];
        let line_number_to_process = get_last_newline_line_number(&self.answer_up_until_now);
        if line_number_to_process.is_none() {
            return;
        }
        let line_number_to_process_until = line_number_to_process.expect("to work") - 1;
        let answer_lines = self
            .answer_up_until_now
            .lines()
            .into_iter()
            .collect::<Vec<_>>();

        let start_index = if self.previous_answer_line_number.is_none() {
            0
        } else {
            self.previous_answer_line_number
                .expect("if_none above to work")
                + 1
        };

        for line_number in start_index..=line_number_to_process_until {
            self.previous_answer_line_number = Some(line_number);
            let answer_line_at_index = answer_lines[line_number];

            match self.code_to_add_block_status.clone() {
                CodeToAddBlockStatus::NoBlock => {
                    if head
                        .iter()
                        .find(|head| **head == answer_line_at_index)
                        .is_some()
                    {
                        self.code_to_add_block_status = CodeToAddBlockStatus::BlockStart;
                        let _ = self.sender.send(CodeBlockEditDelta::EditStarted);
                    }
                    continue;
                }
                CodeToAddBlockStatus::BlockStart => {
                    if !answer_line_at_index.starts_with("```") {
                        let answer_string = format!("```\n{}", answer_line_at_index);
                        self.code_to_add_block_status =
                            CodeToAddBlockStatus::BlockAccumualtor(answer_string.to_owned());
                        let _ = self
                            .sender
                            .send(CodeBlockEditDelta::EditDelta(answer_string.to_owned()));
                    } else {
                        self.code_to_add_block_status =
                            CodeToAddBlockStatus::BlockAccumualtor(answer_line_at_index.to_owned());
                        let _ = self.sender.send(CodeBlockEditDelta::EditDelta(
                            answer_line_at_index.to_owned(),
                        ));
                    }
                }
                CodeToAddBlockStatus::BlockAccumualtor(accumulated) => {
                    if tail
                        .iter()
                        .find(|tail| **tail == answer_line_at_index)
                        .is_some()
                    {
                        if !accumulated.ends_with("```") {
                            let _ = self
                                .sender
                                .send(CodeBlockEditDelta::EditDelta("\n```".to_owned()));
                        }
                        self.code_to_add_block_status = CodeToAddBlockStatus::NoBlock;
                        let _ = self.sender.send(CodeBlockEditDelta::EditEnd);
                        continue;
                    } else {
                        let _ = self.sender.send(CodeBlockEditDelta::EditDelta(
                            "\n".to_owned() + &answer_line_at_index,
                        ));
                        self.code_to_add_block_status = CodeToAddBlockStatus::BlockAccumualtor(
                            accumulated + "\n" + &answer_line_at_index,
                        );
                    }
                }
            }
        }
    }
}

fn get_last_newline_line_number(s: &str) -> Option<usize> {
    s.rfind('\n')
        .map(|last_index| s[..=last_index].chars().filter(|&c| c == '\n').count())
}

#[cfg(test)]
mod tests {
    use super::CodeEditingTool;

    #[test]
    fn test_code_editing() {
        let code = r#"Here is the edited code with the requested change:

<reply>
```python
    def delete(self):
        # sort instance collections
        for model, instances in self.data.items():
            self.data[model] = sorted(instances, key=attrgetter("pk"))

        # if possible, bring the models in an order suitable for databases that
        # don't support transactions or cannot defer constraint checks until the
        # end of a transaction.
        self.sort()
        # number of objects deleted for each model label
        deleted_counter = Counter()

        # Optimize for the case with a single obj and no dependencies
        if len(self.data) == 1 and len(instances) == 1:
            instance = list(instances)[0]
            if self.can_fast_delete(instance):
                with transaction.mark_for_rollback_on_error():
                    count = sql.DeleteQuery(model).delete_batch([instance.pk], self.using)
                return count, {model._meta.label: count}

        with transaction.atomic(using=self.using, savepoint=False):
            # send pre_delete signals
            for model, obj in self.instances_with_model():
                if not model._meta.auto_created:
                    signals.pre_delete.send(
                        sender=model, instance=obj, using=self.using
                    )

            # fast deletes
            for qs in self.fast_deletes:
                count = qs._raw_delete(using=self.using)
                deleted_counter[qs.model._meta.label] += count

            # update fields
            for model, instances_for_fieldvalues in self.field_updates.items():
                for (field, value), instances in instances_for_fieldvalues.items():
                    query = sql.UpdateQuery(model)
                    query.update_batch([obj.pk for obj in instances],
                                        {field.name: value}, self.using)

            # reverse instance collections
            for instances in self.data.values():
                instances.reverse()

            # delete instances
            for model, instances in self.data.items():
                query = sql.DeleteQuery(model)
                pk_list = [obj.pk for obj in instances]
                count = query.delete_batch(pk_list, self.using)
                deleted_counter[model._meta.label] += count

                if not model._meta.auto_created:
                    for obj in instances:
                        signals.post_delete.send(
                            sender=model, instance=obj, using=self.using
                        )

        # update collected instances
        for instances_for_fieldvalues in self.field_updates.values():
            for (field, value), instances in instances_for_fieldvalues.items():
                for obj in instances:
                    setattr(obj, field.attname, value)
        for model, instances in self.data.items():
            for instance in instances:
                setattr(instance, model._meta.pk.attname, None)
        return sum(deleted_counter.values()), dict(deleted_counter)
```
</reply>

The only change made is in the last loop, where we set the primary key attribute of each instance to `None` after deletion:

```python
for model, instances in self.data.items():
    for instance in instances:
        setattr(instance, model._meta.pk.attname, None)
```

This ensures that the primary key of the deleted instances is cleared after the deletion process is complete."#.to_owned();
        let edit_code = CodeEditingTool::edit_code(&code, false, "").expect("to work");
        let better_data = r#"    def delete(self):
        # sort instance collections
        for model, instances in self.data.items():
            self.data[model] = sorted(instances, key=attrgetter(&quot;pk&quot;))

        # if possible, bring the models in an order suitable for databases that
        # don&apos;t support transactions or cannot defer constraint checks until the
        # end of a transaction.
        self.sort()
        # number of objects deleted for each model label
        deleted_counter = Counter()

        # Optimize for the case with a single obj and no dependencies
        if len(self.data) == 1 and len(instances) == 1:
            instance = list(instances)[0]
            if self.can_fast_delete(instance):
                with transaction.mark_for_rollback_on_error():
                    count = sql.DeleteQuery(model).delete_batch([instance.pk], self.using)
                return count, {model._meta.label: count}

        with transaction.atomic(using=self.using, savepoint=False):
            # send pre_delete signals
            for model, obj in self.instances_with_model():
                if not model._meta.auto_created:
                    signals.pre_delete.send(
                        sender=model, instance=obj, using=self.using
                    )

            # fast deletes
            for qs in self.fast_deletes:
                count = qs._raw_delete(using=self.using)
                deleted_counter[qs.model._meta.label] += count

            # update fields
            for model, instances_for_fieldvalues in self.field_updates.items():
                for (field, value), instances in instances_for_fieldvalues.items():
                    query = sql.UpdateQuery(model)
                    query.update_batch([obj.pk for obj in instances],
                                        {field.name: value}, self.using)

            # reverse instance collections
            for instances in self.data.values():
                instances.reverse()

            # delete instances
            for model, instances in self.data.items():
                query = sql.DeleteQuery(model)
                pk_list = [obj.pk for obj in instances]
                count = query.delete_batch(pk_list, self.using)
                deleted_counter[model._meta.label] += count

                if not model._meta.auto_created:
                    for obj in instances:
                        signals.post_delete.send(
                            sender=model, instance=obj, using=self.using
                        )

        # update collected instances
        for instances_for_fieldvalues in self.field_updates.values():
            for (field, value), instances in instances_for_fieldvalues.items():
                for obj in instances:
                    setattr(obj, field.attname, value)
        for model, instances in self.data.items():
            for instance in instances:
                setattr(instance, model._meta.pk.attname, None)
        return sum(deleted_counter.values()), dict(deleted_counter)"#;
        assert_eq!(edit_code, better_data);
    }

    #[test]
    fn parsing_code_edit() {
        let response = r#"
<reply>
<thinking>
The user wants to add comments to the `RequestEvents` enum variants. I will add a comment to each variant explaining its purpose.
</thinking>
<code_edited>
#[derive(Debug, serde::Serialize)]
pub enum RequestEvents {
    /// Indicates the start of a probing interaction.
    ProbingStart,
    /// Signifies the completion of a probe, carrying the probe's response.
    ProbeFinished(RequestEventProbeFinished),
}
</code_edited>
</reply>
        "#
        .to_owned();
        let _ = CodeEditingTool::edit_code(&response, false, "").expect("to work");
    }
}
