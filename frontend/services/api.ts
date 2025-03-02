import axios from 'axios';

const API_BASE_URL = '/api';

// File operations
export const listFiles = async (directoryPath: string) => {
  try {
    const response = await axios.get(`${API_BASE_URL}/file`, {
      params: { directory_path: directoryPath }
    });
    return response.data;
  } catch (error) {
    console.error('Error listing files:', error);
    throw error;
  }
};

export const readFile = async (filePath: string) => {
  try {
    const response = await axios.get(`${API_BASE_URL}/file`, {
      params: { fs_file_path: filePath }
    });
    return response.data;
  } catch (error) {
    console.error('Error reading file:', error);
    throw error;
  }
};

export const editFile = async (filePath: string, content: string) => {
  try {
    const response = await axios.post(`${API_BASE_URL}/file/edit_file`, {
      fs_file_path: filePath,
      content
    });
    return response.data;
  } catch (error) {
    console.error('Error editing file:', error);
    throw error;
  }
};

// Agent operations with streaming support
export const sendAgentMessage = async (
  params: {
    sessionId: string;
    exchangeId: string;
    editorUrl: string;
    query: string;
    userContext: {
      visibleFiles: string[];
      openFiles: string[];
    };
    repoRef: {
      name: string;
      url: string;
    };
    projectLabels: string[];
    rootDirectory: string;
    accessToken: string;
    modelConfiguration: {
      fastModel: string;
      slowModel: string;
    };
    allFiles: string[];
    openFiles: string[];
    shell: string;
  },
  onMessageChunk?: (chunk: string) => void
) => {
  try {
    // If streaming is requested, use fetch with streaming
    if (onMessageChunk) {
      const response = await fetch(`${API_BASE_URL}/agentic/agent_tool_use`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          session_id: params.sessionId,
          exchange_id: params.exchangeId,
          editor_url: params.editorUrl,
          query: params.query,
          user_context: {
            visible_files: params.userContext.visibleFiles,
            open_files: params.userContext.openFiles,
          },
          repo_ref: params.repoRef,
          project_labels: params.projectLabels,
          root_directory: params.rootDirectory,
          access_token: params.accessToken,
          model_configuration: {
            fast_model: params.modelConfiguration.fastModel,
            slow_model: params.modelConfiguration.slowModel,
          },
          all_files: params.allFiles,
          open_files: params.openFiles,
          shell: params.shell,
        }),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      if (!response.body) {
        throw new Error('Response body is null');
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();

      // Read the stream
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        
        const chunk = decoder.decode(value, { stream: true });
        onMessageChunk(chunk);
      }

      return { success: true };
    } else {
      // If no streaming is requested, use axios as before
      const response = await axios.post(`${API_BASE_URL}/agentic/agent_tool_use`, {
        session_id: params.sessionId,
        exchange_id: params.exchangeId,
        editor_url: params.editorUrl,
        query: params.query,
        user_context: {
          visible_files: params.userContext.visibleFiles,
          open_files: params.userContext.openFiles,
        },
        repo_ref: params.repoRef,
        project_labels: params.projectLabels,
        root_directory: params.rootDirectory,
        access_token: params.accessToken,
        model_configuration: {
          fast_model: params.modelConfiguration.fastModel,
          slow_model: params.modelConfiguration.slowModel,
        },
        all_files: params.allFiles,
        open_files: params.openFiles,
        shell: params.shell,
      });
      return response.data;
    }
  } catch (error) {
    console.error('Error sending agent message:', error);
    throw error;
  }
};

// Health check
export const checkHealth = async () => {
  try {
    const response = await axios.get(`${API_BASE_URL}/health`);
    return response.data;
  } catch (error) {
    console.error('Error checking health:', error);
    throw error;
  }
};

// Config
export const getConfig = async () => {
  try {
    const response = await axios.get(`${API_BASE_URL}/config`);
    return response.data;
  } catch (error) {
    console.error('Error getting config:', error);
    throw error;
  }
};

// Version
export const getVersion = async () => {
  try {
    const response = await axios.get(`${API_BASE_URL}/version`);
    return response.data;
  } catch (error) {
    console.error('Error getting version:', error);
    throw error;
  }
};