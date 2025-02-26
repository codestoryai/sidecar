# Sidecar Settings and Model Configuration

This guide explains how to implement model switching and settings configuration in the standalone Sidecar frontend.

## Settings Architecture

The settings system consists of:
- Settings UI components
- State management for settings
- Backend API endpoints for configuration
- Persistent storage

## Implementation

1. Create settings types:
```typescript
// types/settings.ts
interface ModelConfig {
  name: string;
  provider: 'openai' | 'anthropic' | 'local';
  contextLength: number;
  temperature: number;
}

interface SidecarSettings {
  activeModel: string;
  models: ModelConfig[];
  customPrompts: Record<string, string>;
  editorPreferences: {
    theme: string;
    fontSize: number;
    tabSize: number;
  };
}
```

2. Implement settings store:
```typescript
// store/settings.ts
import create from 'zustand';

export const useSettings = create<{
  settings: SidecarSettings;
  updateSettings: (settings: Partial<SidecarSettings>) => void;
}>((set) => ({
  settings: {
    activeModel: 'gpt-4',
    models: [
      {
        name: 'gpt-4',
        provider: 'openai',
        contextLength: 8192,
        temperature: 0.7
      },
      {
        name: 'claude-2',
        provider: 'anthropic',
        contextLength: 100000,
        temperature: 0.7
      }
    ],
    customPrompts: {},
    editorPreferences: {
      theme: 'dark',
      fontSize: 14,
      tabSize: 2
    }
  },
  updateSettings: (newSettings) =>
    set((state) => ({
      settings: { ...state.settings, ...newSettings }
    }))
}));
```

3. Create settings UI:
```typescript
// components/SettingsPanel.tsx
export function SettingsPanel() {
  const { settings, updateSettings } = useSettings();
  
  return (
    <div className="settings-panel">
      <section>
        <h3>Model Selection</h3>
        <select
          value={settings.activeModel}
          onChange={(e) => updateSettings({ activeModel: e.target.value })}
        >
          {settings.models.map(model => (
            <option key={model.name} value={model.name}>
              {model.name}
            </option>
          ))}
        </select>
      </section>
      
      <section>
        <h3>Custom Prompts</h3>
        {/* Prompt editor interface */}
      </section>
      
      <section>
        <h3>Editor Preferences</h3>
        {/* Editor settings controls */}
      </section>
    </div>
  );
}
```

4. Add backend endpoints:
```rust
// src/settings.rs
#[derive(Serialize, Deserialize)]
struct Settings {
    active_model: String,
    models: Vec<ModelConfig>,
    custom_prompts: HashMap<String, String>,
    editor_preferences: EditorPreferences,
}

#[post("/settings")]
async fn update_settings(Json(settings): Json<Settings>) -> impl IntoResponse {
    // Update settings in persistent storage
    store_settings(&settings).await?;
    
    // Reconfigure model clients if needed
    reconfigure_model_clients(&settings.active_model).await?;
    
    Json(settings)
}
```

## Persistent Storage

1. Store settings in local storage:
```typescript
// lib/storage.ts
export function saveSettings(settings: SidecarSettings) {
  localStorage.setItem('sidecar-settings', JSON.stringify(settings));
}

export function loadSettings(): SidecarSettings {
  const saved = localStorage.getItem('sidecar-settings');
  return saved ? JSON.parse(saved) : defaultSettings;
}
```

2. Backend storage:
```rust
// src/storage.rs
pub async fn store_settings(settings: &Settings) -> Result<()> {
    let path = config_dir().join("settings.json");
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(path, json).await?;
    Ok(())
}
```

## Model Integration

1. Model client factory:
```typescript
// lib/models.ts
class ModelClientFactory {
  createClient(config: ModelConfig) {
    switch (config.provider) {
      case 'openai':
        return new OpenAIClient(config);
      case 'anthropic':
        return new AnthropicClient(config);
      case 'local':
        return new LocalModelClient(config);
    }
  }
}
```

2. Model switching:
```typescript
// lib/completion.ts
export async function getCompletion(prompt: string) {
  const { settings } = useSettings.getState();
  const model = settings.models.find(m => m.name === settings.activeModel);
  const client = new ModelClientFactory().createClient(model);
  return client.complete(prompt);
}
```

## Custom Prompts

1. Prompt template system:
```typescript
// lib/prompts.ts
export class PromptTemplate {
  constructor(private template: string) {}

  format(params: Record<string, string>): string {
    return this.template.replace(/\${(\w+)}/g, (_, key) => params[key]);
  }
}
```

2. Prompt management:
```typescript
// components/PromptEditor.tsx
export function PromptEditor() {
  const { settings, updateSettings } = useSettings();
  
  const addPrompt = (name: string, template: string) => {
    updateSettings({
      customPrompts: {
        ...settings.customPrompts,
        [name]: template
      }
    });
  };

  return (
    <div className="prompt-editor">
      {/* Prompt editing interface */}
    </div>
  );
}
```

## Security Considerations

1. Validate settings before applying:
- Check model configurations
- Sanitize custom prompts
- Validate API keys

2. Encrypt sensitive data:
- API keys
- Custom prompts with sensitive content
- User preferences

## Usage Example

```typescript
// pages/index.tsx
export default function Editor() {
  const { settings } = useSettings();
  
  const handleCompletion = async () => {
    const completion = await getCompletion(prompt);
    // Handle completion result
  };
  
  return (
    <div className="editor">
      <SettingsPanel />
      <CodeMirror
        theme={settings.editorPreferences.theme}
        fontSize={settings.editorPreferences.fontSize}
      />
    </div>
  );
}
```

## Best Practices

1. **Settings Validation**
   - Validate all settings changes
   - Provide clear feedback for invalid settings
   - Implement settings migration for updates

2. **Performance**
   - Cache model configurations
   - Lazy load settings UI
   - Debounce settings updates

3. **User Experience**
   - Provide default configurations
   - Include preset prompts
   - Show model capabilities and limitations

4. **Error Handling**
   - Graceful fallbacks for unavailable models
   - Clear error messages
   - Automatic recovery strategies