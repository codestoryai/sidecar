# Building the Standalone Sidecar Frontend

This guide provides a complete implementation for building the standalone Sidecar frontend using Next.js and CodeMirror.

## Project Structure

```
standalone-sidecar/
├── frontend/
│   ├── components/
│   │   ├── Editor/
│   │   ├── Settings/
│   │   └── Git/
│   ├── lib/
│   │   ├── api/
│   │   ├── git/
│   │   └── websocket/
│   ├── pages/
│   └── styles/
└── backend/
    └── src/
        ├── main.rs
        └── handlers/
```

## Getting Started

1. Create the Next.js project:
```bash
npx create-next-app@latest frontend --typescript --tailwind
cd frontend
```

2. Install dependencies:
```bash
npm install @uiw/react-codemirror @codemirror/lang-javascript @codemirror/theme-one-dark
npm install @codemirror/language @codemirror/state @codemirror/view @codemirror/commands
npm install @codemirror/autocomplete @codemirror/lint @codemirror/search @codemirror/language-data
npm install isomorphic-git @monaco-editor/react zustand axios react-query
```

## Core Components Implementation

### 1. Editor Component

```typescript
// components/Editor/Editor.tsx
import { useEffect, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { oneDark } from '@codemirror/theme-one-dark';
import { useSettings } from '@/lib/store/settings';
import { useWebSocket } from '@/lib/hooks/useWebSocket';

export function Editor() {
  const { settings } = useSettings();
  const ws = useWebSocket();
  const [value, setValue] = useState('');
  const [suggestions, setSuggestions] = useState([]);

  const handleChange = async (val: string) => {
    setValue(val);
    ws.send({
      type: 'completion_request',
      payload: {
        code: val,
        cursor: getCursorPosition(),
        language: 'typescript'
      }
    });
  };

  return (
    <div className="h-screen bg-gray-900">
      <CodeMirror
        value={value}
        height="100%"
        theme={oneDark}
        extensions={[javascript()]}
        onChange={handleChange}
        className="text-lg"
      />
      {suggestions.length > 0 && (
        <div className="suggestions-panel">
          {suggestions.map((suggestion, i) => (
            <div key={i} className="suggestion-item">
              {suggestion.text}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
```

### 2. Settings Panel

```typescript
// components/Settings/SettingsPanel.tsx
import { useSettings } from '@/lib/store/settings';

export function SettingsPanel() {
  const { settings, updateSettings } = useSettings();

  return (
    <div className="settings-panel p-4 bg-gray-800 text-white">
      <h2 className="text-xl mb-4">Settings</h2>
      
      <section className="mb-6">
        <h3 className="text-lg mb-2">Model Selection</h3>
        <select 
          value={settings.activeModel}
          onChange={(e) => updateSettings({ activeModel: e.target.value })}
          className="w-full p-2 bg-gray-700 rounded"
        >
          {settings.models.map(model => (
            <option key={model.name} value={model.name}>
              {model.name}
            </option>
          ))}
        </select>
      </section>

      <section className="mb-6">
        <h3 className="text-lg mb-2">Editor Preferences</h3>
        <div className="space-y-2">
          <div>
            <label>Theme</label>
            <select
              value={settings.editorPreferences.theme}
              onChange={(e) => updateSettings({
                editorPreferences: { ...settings.editorPreferences, theme: e.target.value }
              })}
              className="w-full p-2 bg-gray-700 rounded"
            >
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </div>
          <div>
            <label>Font Size</label>
            <input
              type="number"
              value={settings.editorPreferences.fontSize}
              onChange={(e) => updateSettings({
                editorPreferences: { ...settings.editorPreferences, fontSize: parseInt(e.target.value) }
              })}
              className="w-full p-2 bg-gray-700 rounded"
            />
          </div>
        </div>
      </section>
    </div>
  );
}
```

### 3. Git Integration

```typescript
// components/Git/GitPanel.tsx
import { useGit } from '@/lib/hooks/useGit';

export function GitPanel() {
  const { status, addFiles, commit } = useGit();

  const handleCommit = async (message: string) => {
    await commit(message);
  };

  return (
    <div className="git-panel p-4 bg-gray-800 text-white">
      <h2 className="text-xl mb-4">Git Operations</h2>
      
      <div className="mb-4">
        <h3 className="text-lg mb-2">Status</h3>
        <pre className="bg-gray-700 p-2 rounded">
          {JSON.stringify(status, null, 2)}
        </pre>
      </div>

      <div className="mb-4">
        <h3 className="text-lg mb-2">Commit Changes</h3>
        <input
          type="text"
          placeholder="Commit message"
          className="w-full p-2 bg-gray-700 rounded mb-2"
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              handleCommit(e.currentTarget.value);
            }
          }}
        />
      </div>
    </div>
  );
}
```

## State Management

```typescript
// lib/store/settings.ts
import create from 'zustand';

interface Settings {
  activeModel: string;
  models: Array<{
    name: string;
    provider: string;
    contextLength: number;
  }>;
  editorPreferences: {
    theme: string;
    fontSize: number;
    tabSize: number;
  };
}

export const useSettings = create<{
  settings: Settings;
  updateSettings: (settings: Partial<Settings>) => void;
}>((set) => ({
  settings: {
    activeModel: 'gpt-4',
    models: [
      {
        name: 'gpt-4',
        provider: 'openai',
        contextLength: 8192
      },
      {
        name: 'claude-2',
        provider: 'anthropic',
        contextLength: 100000
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
    }))
}));
```

## API Integration

```typescript
// lib/api/sidecar.ts
import axios from 'axios';

const api = axios.create({
  baseURL: 'http://localhost:3001'
});

export const sidecarApi = {
  getCompletions: async (code: string, cursor: number) => {
    const response = await api.post('/completion', {
      code,
      cursor,
      language: 'typescript'
    });
    return response.data;
  },

  updateSettings: async (settings: any) => {
    const response = await api.post('/settings', settings);
    return response.data;
  },

  getStatus: async () => {
    const response = await api.get('/status');
    return response.data;
  }
};
```

## WebSocket Communication

```typescript
// lib/websocket/connection.ts
export class WebSocketConnection {
  private ws: WebSocket;
  private messageHandlers: Map<string, Function>;

  constructor() {
    this.ws = new WebSocket('ws://localhost:3001/ws');
    this.messageHandlers = new Map();
    this.setupHandlers();
  }

  private setupHandlers() {
    this.ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      const handler = this.messageHandlers.get(data.type);
      if (handler) {
        handler(data.payload);
      }
    };
  }

  public send(message: any) {
    this.ws.send(JSON.stringify(message));
  }

  public onMessage(type: string, handler: Function) {
    this.messageHandlers.set(type, handler);
  }
}
```

## Main Application Layout

```typescript
// pages/index.tsx
import { Editor } from '@/components/Editor/Editor';
import { SettingsPanel } from '@/components/Settings/SettingsPanel';
import { GitPanel } from '@/components/Git/GitPanel';

export default function Home() {
  return (
    <div className="flex h-screen">
      <div className="w-3/4">
        <Editor />
      </div>
      <div className="w-1/4 bg-gray-800">
        <SettingsPanel />
        <GitPanel />
      </div>
    </div>
  );
}
```

## Running the Application

1. Start the Sidecar backend:
```bash
cd backend
cargo run --bin webserver
```

2. Start the frontend development server:
```bash
cd frontend
npm run dev
```

The application will be available at `http://localhost:3000`.

## Security Considerations

1. Implement proper authentication
2. Secure WebSocket connections
3. Validate all user inputs
4. Sanitize code before execution
5. Implement rate limiting

## Performance Optimization

1. Implement caching for:
   - Code analysis results
   - Git operations
   - LLM completions
2. Use WebSocket for real-time updates
3. Optimize large file handling
4. Implement proper error boundaries

## Testing

1. Unit tests for components
2. Integration tests for API
3. End-to-end testing
4. Performance testing

## Deployment

1. Build the frontend:
```bash
npm run build
```

2. Build the backend:
```bash
cargo build --release
```

3. Deploy using Docker:
```bash
docker-compose up -d
```

## Future Improvements

1. Collaborative editing
2. Multiple file support
3. Advanced git operations
4. Custom theme support
5. Plugin system