use super::agent_stream::generate_agent_stream;
use super::model_selection::LLMClientConfig;
use super::types::json;
use anyhow::Context;
use llm_prompts::reranking::types::TERMINAL_OUTPUT;
use std::collections::HashSet;

use axum::response::IntoResponse;
use axum::{extract::Query as axumQuery, Extension, Json};
/// We will invoke the agent to get the answer, we are moving to an agent based work
use serde::{Deserialize, Serialize};

use crate::agent::types::AgentAction;
use crate::agent::types::CodeSpan;
use crate::agent::types::ConversationMessage;
use crate::agent::types::{Agent, VariableInformation as AgentVariableInformation};
use crate::application::application::Application;
use crate::chunking::text_document::Position as DocumentPosition;
use crate::repo::types::RepoRef;
use crate::reporting::posthog::client::PosthogEvent;
use crate::user_context::types::{UserContext, VariableInformation, VariableType};

use super::types::ApiResponse;
use super::types::Result;

fn default_thread_id() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchInformation {
    pub query: String,
    pub reporef: RepoRef,
    #[serde(default = "default_thread_id")]
    pub thread_id: uuid::Uuid,
    pub model_config: LLMClientConfig,
}

impl ApiResponse for SearchInformation {}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct SearchResponse {
    pub query: String,
    pub answer: String,
}

impl ApiResponse for SearchResponse {}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum SearchEvents {
    SearchEvent(),
}

pub async fn search_agent(
    axumQuery(SearchInformation {
        query,
        reporef,
        thread_id,
        model_config,
    }): axumQuery<SearchInformation>,
    Extension(app): Extension<Application>,
) -> Result<impl IntoResponse> {
    let reranker = app.reranker.clone();
    let chat_broker = app.chat_broker.clone();
    let llm_tokenizer = app.llm_tokenizer.clone();
    let session_id = uuid::Uuid::new_v4();
    let llm_broker = app.llm_broker.clone();
    let sql_db = app.sql.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(100);
    let action = AgentAction::Query(query.clone());
    let previous_conversation_message =
        ConversationMessage::load_from_db(sql_db.clone(), &reporef, thread_id)
            .await
            .expect("loading from db to never fail");
    let agent = Agent::prepare_for_search(
        app,
        reporef,
        session_id,
        &query,
        llm_broker,
        thread_id,
        sql_db,
        previous_conversation_message,
        sender,
        Default::default(),
        model_config,
        llm_tokenizer,
        chat_broker,
        reranker,
    );

    generate_agent_stream(agent, action, receiver).await
}

// Here we are going to provide a hybrid search index which combines both the
// lexical and the semantic search together
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HybridSearchQuery {
    query: String,
    repo: RepoRef,
    model_config: LLMClientConfig,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HybridSearchResponse {
    session_id: uuid::Uuid,
    query: String,
    code_spans: Vec<CodeSpan>,
}

impl ApiResponse for HybridSearchResponse {}

/// What's hybrid search? Hybrid search combines the best things about both semantic
/// and lexical search along with statistics from the git log to generate the
/// best code spans which are relevant
pub async fn hybrid_search(
    axumQuery(HybridSearchQuery {
        query,
        repo,
        model_config,
    }): axumQuery<HybridSearchQuery>,
    Extension(app): Extension<Application>,
) -> Result<impl IntoResponse> {
    // Here we want to do the following:
    // - do a semantic search (normalize it to a score between 0.5 -> 1)
    // - do a lexical search (normalize it to a score between 0.5 -> 1)
    // - get statistics from the git log (normalize it to a score between 0.5 -> 1)
    // hand-waving the numbers here for whatever works for now
    // - final score -> git_log_score * 4 + lexical_search * 2.5 + semantic_search_score
    // - combine the score as following
    let reranker = app.reranker.clone();
    let chat_broker = app.chat_broker.clone();
    let llm_broker = app.llm_broker.clone();
    let llm_tokenizer = app.llm_tokenizer.clone();
    let session_id = uuid::Uuid::new_v4();
    let conversation_id = uuid::Uuid::new_v4();
    let sql_db = app.sql.clone();
    let (sender, _) = tokio::sync::mpsc::channel(100);
    let mut agent = Agent::prepare_for_semantic_search(
        app,
        repo,
        session_id,
        &query,
        llm_broker,
        conversation_id,
        sql_db,
        vec![], // we don't have a previous conversation message here
        sender,
        Default::default(),
        model_config,
        llm_tokenizer,
        chat_broker,
        reranker,
    );
    // let hybrid_search_results = agent.code_search_hybrid(&query).await.unwrap_or(vec![]);
    Ok(json(HybridSearchResponse {
        session_id: uuid::Uuid::new_v4(),
        query,
        code_spans: vec![],
    }))
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ExplainRequest {
    query: String,
    relative_path: String,
    start_line: u64,
    end_line: u64,
    repo_ref: RepoRef,
    #[serde(default = "default_thread_id")]
    thread_id: uuid::Uuid,
    model_config: LLMClientConfig,
}

/// We are going to handle the explain function here, but its going to be very
/// bare-bones right now. We don't give the user the option to explore or do
/// more things with the agent yet, ideal explain feature will be when the user
/// gets to explore the repository or maybe that can be a different UX like the
/// crawler
pub async fn explain(
    axumQuery(ExplainRequest {
        query,
        relative_path,
        start_line,
        end_line,
        repo_ref,
        thread_id,
        model_config,
    }): axumQuery<ExplainRequest>,
    Extension(app): Extension<Application>,
) -> Result<impl IntoResponse> {
    let reranker = app.reranker.clone();
    let chat_broker = app.chat_broker.clone();
    let llm_broker = app.llm_broker.clone();
    let llm_tokenizer = app.llm_tokenizer.clone();
    let user_id = app.user_id.to_owned();
    let posthog_client = app.posthog_client.clone();
    let sql_db = app.sql.clone();
    // let file_content = app
    //     .indexes
    //     .file
    //     .get_by_path(&relative_path, &repo_ref)
    //     .await
    //     .context("file retrieval failed")?
    //     .context("requested file not found")?
    //     .content;

    let mut previous_messages =
        ConversationMessage::load_from_db(app.sql.clone(), &repo_ref, thread_id)
            .await
            .expect("loading from db to never fail");

    // let snippet = file_content
    //     .lines()
    //     .skip(start_line.try_into().expect("conversion_should_not_fail"))
    //     .take(
    //         (end_line - start_line)
    //             .try_into()
    //             .expect("conversion_should_not_fail"),
    //     )
    //     .collect::<Vec<_>>()
    //     .join("\n");
    let snippet = "".to_owned();

    let mut conversation_message = ConversationMessage::explain_message(
        thread_id,
        crate::agent::types::AgentState::Explain,
        query,
    );

    let code_span = CodeSpan {
        file_path: relative_path.to_owned(),
        alias: 0,
        start_line,
        end_line,
        data: snippet,
        score: Some(1.0),
    };
    conversation_message.add_code_spans(code_span.clone());
    conversation_message.add_path(relative_path);

    previous_messages.push(conversation_message);

    let action = AgentAction::Answer { paths: vec![0] };

    let (sender, receiver) = tokio::sync::mpsc::channel(100);

    let session_id = uuid::Uuid::new_v4();

    let sql = app.sql.clone();
    let editor_parsing = Default::default();

    let agent = Agent {
        application: app,
        reporef: repo_ref,
        session_id,
        conversation_messages: previous_messages,
        llm_broker,
        sql_db: sql,
        sender,
        user_context: None,
        project_labels: vec![],
        editor_parsing,
        model_config,
        llm_tokenizer,
        chat_broker,
        reranker,
        system_instruction: None,
    };

    generate_agent_stream(agent, action, receiver).await
}

impl Into<crate::agent::types::VariableType> for VariableType {
    fn into(self) -> crate::agent::types::VariableType {
        match self {
            VariableType::File => crate::agent::types::VariableType::File,
            VariableType::CodeSymbol => crate::agent::types::VariableType::CodeSymbol,
            VariableType::Selection => crate::agent::types::VariableType::Selection,
        }
    }
}

impl VariableInformation {
    pub fn to_agent_type(self) -> AgentVariableInformation {
        AgentVariableInformation {
            start_position: DocumentPosition::new(
                self.start_position.line(),
                self.start_position.column(),
                0,
            ),
            end_position: DocumentPosition::new(
                self.end_position.line(),
                self.end_position.column(),
                0,
            ),
            fs_file_path: self.fs_file_path,
            name: self.name,
            variable_type: self.variable_type.into(),
            content: self.content,
            language: self.language,
        }
    }

    pub fn from_user_active_window(active_window: &ActiveWindowData) -> Self {
        Self {
            start_position: DocumentPosition::new(
                active_window.start_line.try_into().unwrap(),
                0,
                0,
            ),
            end_position: DocumentPosition::new(
                active_window.end_line.try_into().unwrap(),
                1000,
                0,
            ),
            fs_file_path: active_window.file_path.to_owned(),
            name: "active_window".to_owned(),
            variable_type: VariableType::Selection,
            content: active_window.visible_range_content.to_owned(),
            language: active_window.language.to_owned(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ActiveWindowData {
    pub file_path: String,
    pub file_content: String,
    pub language: String,
    pub visible_range_content: String,
    // start line and end line here refer to the range of the active window for
    // the user
    pub start_line: usize,
    pub end_line: usize,
}

impl UserContext {
    fn merge_from_previous(mut self, previous: Option<&UserContext>) -> Self {
        // Here we try and merge the user contexts together, if we have something
        match previous {
            Some(previous_user_context) => {
                let previous_file_content = &previous_user_context.file_content_map;
                let previous_user_variables = &previous_user_context.variables;
                let previous_terminal_selection = &previous_user_context.terminal_selection;
                // We want to merge the variables together, but keep the unique
                // ones only
                // TODO(skcd): We should be filtering on variables here, but for
                // now we ball 🖲️
                self.variables
                    .extend(previous_user_variables.to_vec().into_iter());
                // We want to merge the file content map together, and only keep
                // the unique ones and the new file content map we are getting if
                // there are any repetitions
                let mut file_content_set: HashSet<String> = HashSet::new();
                self.file_content_map.iter().for_each(|file_content| {
                    file_content_set.insert(file_content.file_path.to_owned());
                });
                // Look at the previous ones and add those which are missing
                previous_file_content.into_iter().for_each(|file_content| {
                    if !file_content_set.contains(&file_content.file_path) {
                        self.file_content_map.push(file_content.clone());
                    }
                });
                // yolo merge the terminal outputs we are getting, here we disregard
                // the previous terminal message and only keep the current one
                if self.terminal_selection.is_none() {
                    self.terminal_selection = previous_terminal_selection.clone();
                }
                self
            }
            None => self,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct FollowupChatRequest {
    pub query: String,
    pub repo_ref: RepoRef,
    pub thread_id: uuid::Uuid,
    pub user_context: UserContext,
    pub project_labels: Vec<String>,
    pub active_window_data: Option<ActiveWindowData>,
    pub model_config: LLMClientConfig,
    pub system_instruction: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepContextForView {
    pub repo_ref: RepoRef,
    pub precise_context: Vec<PreciseContext>,
    pub cursor_position: Option<CursorPosition>,
    pub current_view_port: Option<CurrentViewPort>,
    pub language: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionSnippet {
    pub context: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreciseContext {
    pub symbol: Symbol,
    pub hover_text: Vec<String>,
    pub definition_snippet: DefinitionSnippet,
    pub fs_file_path: String,
    pub relative_file_path: String,
    pub range: Range,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuzzy_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPosition {
    pub start_position: Position,
    pub end_position: Position,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentViewPort {
    pub start_position: Position,
    pub end_position: Position,
    pub relative_path: String,
    pub fs_file_path: String,
    pub text_on_screen: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_line: usize,
    pub start_character: usize,
    pub end_line: usize,
    pub end_character: usize,
}

pub async fn followup_chat(
    Extension(app): Extension<Application>,
    Json(FollowupChatRequest {
        query,
        repo_ref,
        thread_id,
        user_context,
        project_labels,
        active_window_data,
        model_config,
        system_instruction,
    }): Json<FollowupChatRequest>,
) -> Result<impl IntoResponse> {
    let session_id = uuid::Uuid::new_v4();
    let user_id = app.user_id.to_owned();
    let mut event = PosthogEvent::new("model_config");
    let _ = event.insert_prop("config", model_config.logging_config());
    let _ = event.insert_prop("user_id", user_id);
    let _ = app.posthog_client.capture(event).await;
    // Here we do something special, if the user is asking a followup question
    // we just look at the previous conversation message the thread belonged
    // to and use that as context for grounding the agent response. In the future
    // we can obviously add more context using @ symbols etc
    let reranker = app.reranker.clone();
    let chat_broker = app.chat_broker.clone();
    let llm_broker = app.llm_broker.clone();
    let llm_tokenizer = app.llm_tokenizer.clone();
    let sql_db = app.sql.clone();
    let mut previous_messages =
        ConversationMessage::load_from_db(sql_db.clone(), &repo_ref, thread_id)
            .await
            .unwrap_or_default();
    let last_user_context = previous_messages
        .last()
        .map(|previous_message| previous_message.get_user_context());

    let user_context = user_context.merge_from_previous(last_user_context);

    let mut conversation_message = ConversationMessage::general_question(
        thread_id,
        crate::agent::types::AgentState::FollowupChat,
        query.to_owned(),
    );

    // Add the path for the active window to the conversation message as well
    if let Some(active_window_data) = &active_window_data {
        conversation_message.add_path(active_window_data.file_path.to_owned());
    }
    conversation_message.set_user_context(user_context.clone());
    conversation_message.set_active_window(active_window_data);

    // We add all the paths which we are going to get into the conversation message
    // so that we can use that for the next followup question
    user_context
        .file_content_map
        .iter()
        .for_each(|file_content_value| {
            conversation_message.add_path(file_content_value.file_path.to_owned());
        });

    // If there is terminal selection we also want to set the path for that
    if user_context.terminal_selection.is_some() {
        conversation_message.add_path(TERMINAL_OUTPUT.to_owned());
    }

    // also add the paths for the folders
    user_context.folder_paths().iter().for_each(|folder_path| {
        conversation_message.add_path(folder_path.to_owned());
    });

    // We also want to add the file path for the active window if it's not already there
    let file_path_len = conversation_message.get_paths().len();
    previous_messages.push(conversation_message);

    let (sender, receiver) = tokio::sync::mpsc::channel(100);

    // If this is a followup, right now we don't take in any additional context,
    // but only use the one from our previous conversation
    let action = AgentAction::Answer {
        paths: (0..file_path_len).collect(),
    };

    let agent = Agent::prepare_for_followup(
        app,
        repo_ref,
        session_id,
        llm_broker,
        sql_db,
        previous_messages,
        sender,
        user_context,
        project_labels,
        Default::default(),
        model_config,
        llm_tokenizer,
        chat_broker,
        reranker,
        system_instruction,
    );

    generate_agent_stream(agent, action, receiver).await
}

#[cfg(test)]
mod tests {
    use crate::webserver::model_selection::LLMClientConfig;

    use super::FollowupChatRequest;
    use serde_json;

    #[test]
    fn test_parsing() {
        let input_string = r#"
{"repo_ref":"local/c:\\Users\\keert\\pifuhd","query":"tell me","thread_id":"b265857b-9bf5-4db4-897c-a07d1c4c3b67","user_context":{"variables":[],"file_content_map":[]},"project_labels":["python","pip"],"active_window_data":{"file_path":"c:\\Users\\keert\\pifuhd\\lib\\colab_util.py","file_content":"'''\r\nMIT License\r\n\r\nCopyright (c) 2019 Shunsuke Saito, Zeng Huang, and Ryota Natsume\r\n\r\nPermission is hereby granted, free of charge, to any person obtaining a copy\r\nof this software and associated documentation files (the \"Software\"), to deal\r\nin the Software without restriction, including without limitation the rights\r\nto use, copy, modify, merge, publish, distribute, sublicense, and/or sell\r\ncopies of the Software, and to permit persons to whom the Software is\r\nfurnished to do so, subject to the following conditions:\r\n\r\nThe above copyright notice and this permission notice shall be included in all\r\ncopies or substantial portions of the Software.\r\n\r\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\r\nIMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\r\nFITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\r\nAUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\r\nLIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\r\nOUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\r\nSOFTWARE.\r\n'''\r\nimport io\r\nimport os\r\nimport torch\r\nfrom skimage.io import imread\r\nimport numpy as np\r\nimport cv2\r\nfrom tqdm import tqdm_notebook as tqdm\r\nimport base64\r\nfrom IPython.display import HTML\r\n\r\n# Util function for loading meshes\r\nfrom pytorch3d.io import load_objs_as_meshes\r\n\r\nfrom IPython.display import HTML\r\nfrom base64 import b64encode\r\n\r\n# Data structures and functions for rendering\r\nfrom pytorch3d.structures import Meshes\r\nfrom pytorch3d.renderer import (\r\n    look_at_view_transform,\r\n    OpenGLOrthographicCameras, \r\n    PointLights, \r\n    DirectionalLights, \r\n    Materials, \r\n    RasterizationSettings, \r\n    MeshRenderer, \r\n    MeshRasterizer,  \r\n    HardPhongShader,\r\n    TexturesVertex\r\n)\r\n\r\ndef set_renderer():\r\n    # Setup\r\n    device = torch.device(\"cuda:0\")\r\n    torch.cuda.set_device(device)\r\n\r\n    # Initialize an OpenGL perspective camera.\r\n    R, T = look_at_view_transform(2.0, 0, 180) \r\n    cameras = OpenGLOrthographicCameras(device=device, R=R, T=T)\r\n\r\n    raster_settings = RasterizationSettings(\r\n        image_size=512, \r\n        blur_radius=0.0, \r\n        faces_per_pixel=1, \r\n        bin_size = None, \r\n        max_faces_per_bin = None\r\n    )\r\n\r\n    lights = PointLights(device=device, location=((2.0, 2.0, 2.0),))\r\n\r\n    renderer = MeshRenderer(\r\n        rasterizer=MeshRasterizer(\r\n            cameras=cameras, \r\n            raster_settings=raster_settings\r\n        ),\r\n        shader=HardPhongShader(\r\n            device=device, \r\n            cameras=cameras,\r\n            lights=lights\r\n        )\r\n    )\r\n    return renderer\r\n\r\ndef get_verts_rgb_colors(obj_path):\r\n  rgb_colors = []\r\n\r\n  f = open(obj_path)\r\n  lines = f.readlines()\r\n  for line in lines:\r\n    ls = line.split(' ')\r\n    if len(ls) == 7:\r\n      rgb_colors.append(ls[-3:])\r\n\r\n  return np.array(rgb_colors, dtype='float32')[None, :, :]\r\n\r\ndef generate_video_from_obj(obj_path, image_path, video_path, renderer):\r\n    input_image = cv2.imread(image_path)\r\n    input_image = input_image[:,:input_image.shape[1]//3]\r\n    input_image = cv2.resize(input_image, (512,512))\r\n\r\n    # Setup\r\n    device = torch.device(\"cuda:0\")\r\n    torch.cuda.set_device(device)\r\n\r\n    # Load obj file\r\n    verts_rgb_colors = get_verts_rgb_colors(obj_path)\r\n    verts_rgb_colors = torch.from_numpy(verts_rgb_colors).to(device)\r\n    textures = TexturesVertex(verts_features=verts_rgb_colors)\r\n    # wo_textures = TexturesVertex(verts_features=torch.ones_like(verts_rgb_colors)*0.75)\r\n\r\n    # Load obj\r\n    mesh = load_objs_as_meshes([obj_path], device=device)\r\n\r\n    # Set mesh\r\n    vers = mesh._verts_list\r\n    faces = mesh._faces_list\r\n    mesh_w_tex = Meshes(vers, faces, textures)\r\n    # mesh_wo_tex = Meshes(vers, faces, wo_textures)\r\n\r\n    # create VideoWriter\r\n    fourcc = cv2. VideoWriter_fourcc(*'MP4V')\r\n    out = cv2.VideoWriter(video_path, fourcc, 20.0, (1024,512))\r\n\r\n    for i in tqdm(range(90)):\r\n        R, T = look_at_view_transform(1.8, 0, i*4, device=device)\r\n        images_w_tex = renderer(mesh_w_tex, R=R, T=T)\r\n        images_w_tex = np.clip(images_w_tex[0, ..., :3].cpu().numpy(), 0.0, 1.0)[:, :, ::-1] * 255\r\n        # images_wo_tex = renderer(mesh_wo_tex, R=R, T=T)\r\n        # images_wo_tex = np.clip(images_wo_tex[0, ..., :3].cpu().numpy(), 0.0, 1.0)[:, :, ::-1] * 255\r\n        image = np.concatenate([input_image, images_w_tex], axis=1)\r\n        out.write(image.astype('uint8'))\r\n    out.release()\r\n\r\ndef video(path):\r\n    mp4 = open(path,'rb').read()\r\n    data_url = \"data:video/mp4;base64,\" + b64encode(mp4).decode()\r\n    return HTML('<video width=500 controls loop> <source src=\"%s\" type=\"video/mp4\"></video>' % data_url)\r\n","visible_range_content":"            raster_settings=raster_settings\r\n        ),\r\n        shader=HardPhongShader(\r\n            device=device, \r\n            cameras=cameras,\r\n            lights=lights\r\n        )\r\n    )\r\n    return renderer\r\n\r\ndef get_verts_rgb_colors(obj_path):\r\n  rgb_colors = []\r\n\r\n  f = open(obj_path)\r\n  lines = f.readlines()\r\n  for line in lines:\r\n    ls = line.split(' ')\r\n    if len(ls) == 7:\r\n      rgb_colors.append(ls[-3:])\r\n\r\n  return np.array(rgb_colors, dtype='float32')[None, :, :]\r\n\r\ndef generate_video_from_obj(obj_path, image_path, video_path, renderer):\r\n    input_image = cv2.imread(image_path)\r\n    input_image = input_image[:,:input_image.shape[1]//3]\r\n    input_image = cv2.resize(input_image, (512,512))\r\n\r\n    # Setup\r\n    device = torch.device(\"cuda:0\")\r\n    torch.cuda.set_device(device)\r\n\r\n    # Load obj file\r\n    verts_rgb_colors = get_verts_rgb_colors(obj_path)\r\n    verts_rgb_colors = torch.from_numpy(verts_rgb_colors).to(device)\r\n    textures = TexturesVertex(verts_features=verts_rgb_colors)\r\n    # wo_textures = TexturesVertex(verts_features=torch.ones_like(verts_rgb_colors)*0.75)\r\n\r\n    # Load obj\r\n    mesh = load_objs_as_meshes([obj_path], device=device)","start_line":77,"end_line":115,"language":"python"},"openai_key":null,"model_config":{"slow_model":"Gpt4","fast_model":"Gpt4","models":{"Gpt4":{"context_length":8192,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"GPT3_5_16k":{"context_length":16385,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"DeepSeekCoder1.3BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"DeepSeekCoder6BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"ClaudeOpus":{"context_length":200000,"temperature":0.2,"provider":"Anthropic"},"ClaudeSonnet":{"context_length":200000,"temperature":0.2,"provider":"Anthropic"}},"providers":["CodeStory",{"Ollama":{}},{"Anthropic":{"api_key":"soemthing"}}]},"user_id":"keert"}
        "#;
        let parsed_response = serde_json::from_str::<FollowupChatRequest>(&input_string);
        let model_config = r#"
        {"slow_model":"Gpt4","fast_model":"Gpt4","models":{"Gpt4":{"context_length":8192,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"GPT3_5_16k":{"context_length":16385,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"DeepSeekCoder1.3BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"DeepSeekCoder6BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"ClaudeOpus":{"context_length":200000,"temperature":0.2,"provider":"Anthropic"},"ClaudeSonnet":{"context_length":200000,"temperature":0.2,"provider":"Anthropic"}},"providers":["CodeStory",{"Ollama":{}},{"Anthropic":{"api_key":"soemthing"}}]}
        "#.to_owned();
        let parsed_model_config = serde_json::from_str::<LLMClientConfig>(&model_config);
        dbg!(&parsed_response);
        dbg!(&parsed_model_config);
        assert!(parsed_model_config.is_ok());
        assert!(parsed_response.is_ok());
    }
}
