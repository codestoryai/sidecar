# Building a Standalone Sidecar with Next.js and CodeMirror

This guide explains how to create a standalone version of Sidecar using Next.js and CodeMirror, allowing you to have AI-powered code editing capabilities without VSCode.

## Architecture Overview

The standalone version will consist of:
- Next.js frontend with CodeMirror for code editing
- Rust backend (modified Sidecar) for AI operations
- Git integration for version control
- WebSocket communication between frontend and backend

## Prerequisites

- Node.js 18+
- Rust 1.73+
- Git

## Backend Setup

1. Create a modified version of Sidecar:
```rust
// src/main.rs
use axum::{
    routing::{get, post},
    Router, Json,
    extract::WebSocketUpgrade,
};
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/completion", post(completion_handler))
        .layer(CorsLayer::permissive());

    axum::Server::bind(&"0.0.0.0:3001".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

2. Expose key Sidecar functionality:
- Code completion
- Symbol analysis
- Repository mapping
- Git operations

## Frontend Setup

1. Create a Next.js project:
```bash
npx create-next-app@latest standalone-sidecar --typescript
```

2. Install dependencies:
```bash
npm install @uiw/react-codemirror @codemirror/lang-javascript @codemirror/theme-one-dark isomorphic-git
```

3. Create the editor component:
```typescript
// components/Editor.tsx
import { useEffect, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { oneDark } from '@codemirror/theme-one-dark';

export default function Editor() {
  const [value, setValue] = useState('');
  const [suggestions, setSuggestions] = useState([]);
  
  const onChange = async (val: string) => {
    setValue(val);
    // Connect to Sidecar backend for completions
    const response = await fetch('http://localhost:3001/completion', {
      method: 'POST',
      body: JSON.stringify({ code: val, cursor: getCursorPosition() })
    });
    const completions = await response.json();
    setSuggestions(completions);
  };

  return (
    <div className="editor-container">
      <CodeMirror
        value={value}
        height="100vh"
        theme={oneDark}
        extensions={[javascript()]}
        onChange={onChange}
      />
      {suggestions.length > 0 && (
        <div className="suggestions">
          {/* Render completion suggestions */}
        </div>
      )}
    </div>
  );
}
```

## Git Integration

1. Implement Git operations using isomorphic-git:
```typescript
// lib/git.ts
import { promises as fs } from 'fs';
import git from 'isomorphic-git';
import http from 'isomorphic-git/http/web';

export async function initRepo(path: string) {
  await git.init({ fs, dir: path });
}

export async function commitChanges(path: string, message: string) {
  await git.add({ fs, dir: path, filepath: '.' });
  await git.commit({
    fs,
    dir: path,
    message,
    author: {
      name: 'Standalone Sidecar',
      email: 'sidecar@example.com'
    }
  });
}
```

## WebSocket Communication

1. Set up WebSocket connection for real-time updates:
```typescript
// lib/websocket.ts
export class SidecarConnection {
  private ws: WebSocket;

  constructor() {
    this.ws = new WebSocket('ws://localhost:3001/ws');
    this.ws.onmessage = this.handleMessage;
  }

  private handleMessage = (event: MessageEvent) => {
    const data = JSON.parse(event.data);
    // Handle different message types (completions, diagnostics, etc.)
  };

  public sendMessage(type: string, payload: any) {
    this.ws.send(JSON.stringify({ type, payload }));
  }
}
```

## Key Features to Implement

1. **Code Intelligence**
   - Syntax highlighting
   - Code completion
   - Error diagnostics
   - Symbol navigation

2. **Git Integration**
   - File tracking
   - Commit management
   - Branch operations
   - Remote repository sync

3. **AI Features**
   - Code completion
   - Documentation generation
   - Code explanations
   - Refactoring suggestions

## Deployment

1. Build the Rust backend:
```bash
cargo build --release
```

2. Build the Next.js frontend:
```bash
npm run build
```

3. Run both services:
```bash
# Terminal 1
./target/release/standalone-sidecar

# Terminal 2
npm run start
```

## Security Considerations

1. Implement proper authentication
2. Secure WebSocket connections
3. Sanitize code inputs
4. Handle file permissions carefully
5. Validate Git operations

## Performance Optimization

1. Implement caching for:
   - Code analysis results
   - Git operations
   - AI completions

2. Use WebAssembly for heavy computations

3. Optimize WebSocket communication with:
   - Message batching
   - Compression
   - Rate limiting

## Limitations

- Browser limitations for file system access
- Reduced IDE features compared to VSCode
- Network latency for AI operations
- Limited debugging capabilities

## Future Improvements

1. Support for more languages
2. Enhanced Git workflow
3. Collaborative editing
4. Offline capabilities
5. Custom AI model integration