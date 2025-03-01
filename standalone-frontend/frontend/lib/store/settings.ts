import create from 'zustand';

interface EditorPreferences {
  theme: string;
  fontSize: number;
  tabSize: number;
}

interface Model {
  name: string;
  provider: string;
  contextLength: number;
}

interface Settings {
  activeModel: string;
  models: Model[];
  editorPreferences: EditorPreferences;
}

interface SettingsStore {
  settings: Settings;
  updateSettings: (settings: Partial<Settings>) => void;
  updateEditorPreferences: (prefs: Partial<EditorPreferences>) => void;
  setActiveModel: (modelName: string) => void;
}

export const useSettings = create<SettingsStore>((set) => ({
  settings: {
    activeModel: 'Gpt4',
    models: [
      // OpenAI Models
      {
        name: 'GPT3_5_16k',
        provider: 'OpenAI',
        contextLength: 16385
      },
      {
        name: 'Gpt4',
        provider: 'OpenAI',
        contextLength: 8192
      },
      {
        name: 'Gpt4_32k',
        provider: 'OpenAI',
        contextLength: 32768
      },
      {
        name: 'Gpt4Turbo',
        provider: 'OpenAI',
        contextLength: 128000
      },
      {
        name: 'Gpt4O',
        provider: 'OpenAI',
        contextLength: 128000
      },
      {
        name: 'Gpt4OMini',
        provider: 'OpenAI',
        contextLength: 128000
      },
      // TogetherAI Models
      {
        name: 'Mixtral',
        provider: 'TogetherAI',
        contextLength: 32768
      },
      {
        name: 'MistralInstruct',
        provider: 'TogetherAI',
        contextLength: 8192
      },
      {
        name: 'CodeLlama13BInstruct',
        provider: 'TogetherAI',
        contextLength: 100000
      },
      {
        name: 'CodeLlama7BInstruct',
        provider: 'TogetherAI',
        contextLength: 100000
      },
      {
        name: 'DeepSeekCoder33BInstruct',
        provider: 'TogetherAI',
        contextLength: 8192
      },
      // FireworksAI Models
      {
        name: 'Llama3_1_8bInstruct',
        provider: 'FireworksAI',
        contextLength: 8192
      },
      {
        name: 'Llama3_1_70bInstruct',
        provider: 'FireworksAI',
        contextLength: 100000
      },
      // Anthropic Models
      {
        name: 'ClaudeOpus',
        provider: 'Anthropic',
        contextLength: 100000
      },
      {
        name: 'ClaudeSonnet',
        provider: 'Anthropic',
        contextLength: 100000
      },
      {
        name: 'ClaudeHaiku',
        provider: 'Anthropic',
        contextLength: 100000
      },
      // Google Models
      {
        name: 'GeminiPro',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      {
        name: 'GeminiProFlash',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      {
        name: 'Gemini2_0Flash',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      {
        name: 'Gemini2_0FlashExperimental',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      {
        name: 'Gemini2_0FlashThinkingExperimental',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      {
        name: 'Gemini2_0Pro',
        provider: 'GoogleAIStudio',
        contextLength: 32768
      },
      // O1 Models (via OpenRouter)
      {
        name: 'O1Preview',
        provider: 'OpenRouter',
        contextLength: 8192
      },
      {
        name: 'O1Mini',
        provider: 'OpenRouter',
        contextLength: 8192
      },
      {
        name: 'O1',
        provider: 'OpenRouter',
        contextLength: 8192
      },
      {
        name: 'O3MiniHigh',
        provider: 'OpenRouter',
        contextLength: 8192
      }
    ],
    editorPreferences: {
      theme: 'dark',
      fontSize: 14,
      tabSize: 2
    }
  },
  updateSettings: (newSettings) =>
    set((state) => ({
      settings: { ...state.settings, ...newSettings }
    })),
  updateEditorPreferences: (prefs) =>
    set((state) => ({
      settings: {
        ...state.settings,
        editorPreferences: { ...state.settings.editorPreferences, ...prefs }
      }
    })),
  setActiveModel: (modelName) =>
    set((state) => ({
      settings: { ...state.settings, activeModel: modelName }
    }))
}));