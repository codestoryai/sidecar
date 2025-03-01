# Sidecar API Documentation

This guide documents the REST and WebSocket APIs exposed by the Sidecar backend.

## Base URL

- Development: `http://localhost:3001`
- Production: Configure as needed

## Authentication

All API endpoints require authentication via Bearer token:
```http
Authorization: Bearer <your_token>
```

## REST Endpoints

### Code Completion

`POST /completion`

Request code completions based on current context.

**Request:**
```json
{
  "code": "function hello() {",
  "cursor": { "line": 0, "character": 16 },
  "language": "typescript",
  "context": {
    "filePath": "src/hello.ts",
    "projectRoot": "/path/to/project"
  }
}
```

**Response:**
```json
{
  "completions": [
    {
      "text": "console.log('Hello');",
      "score": 0.95
    }
  ]
}
```

### Settings Management

`GET /settings`

Retrieve current settings.

**Response:**
```json
{
  "activeModel": "gpt-4",
  "models": [...],
  "editorPreferences": {...}
}
```

`POST /settings`

Update settings configuration.

**Request:**
```json
{
  "activeModel": "claude-2",
  "editorPreferences": {
    "theme": "dark"
  }
}
```

### Repository Analysis

`POST /analyze`

Analyze repository structure and dependencies.

**Request:**
```json
{
  "path": "/path/to/repo",
  "excludePatterns": ["node_modules", "dist"]
}
```

## WebSocket API

Connect to `ws://localhost:3001/ws`

### Message Types

1. **Code Intelligence:**
```json
{
  "type": "codeIntel",
  "action": "completion",
  "payload": {
    "code": "...",
    "cursor": {...}
  }
}
```

2. **File Changes:**
```json
{
  "type": "fileChange",
  "action": "edit",
  "payload": {
    "path": "src/main.ts",
    "changes": [...]
  }
}
```

### Error Responses

All errors follow this format:
```json
{
  "error": {
    "code": "INVALID_REQUEST",
    "message": "Invalid completion request format"
  }
}
```

## Rate Limiting

- 60 requests per minute for completion endpoints
- 120 requests per minute for other endpoints
- WebSocket messages: 10 per second

## Best Practices

1. **Error Handling**
   - Always check response status codes
   - Implement exponential backoff for retries
   - Handle WebSocket reconnection gracefully

2. **Performance**
   - Batch related requests when possible
   - Cache completion results locally
   - Reuse WebSocket connections

3. **Security**
   - Never send sensitive data in URLs
   - Validate all input on client-side
   - Use HTTPS in production