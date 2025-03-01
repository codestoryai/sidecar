# Sidecar Standalone Frontend

A standalone frontend implementation for Sidecar using Next.js and CodeMirror.

## Features

- Code editing with CodeMirror
- AI-powered code completions
- Git integration
- Model switching and configuration
- Real-time updates via WebSocket
- Dark mode support

## Prerequisites

- Node.js 18+
- npm or yarn
- Sidecar backend running on port 3001

## Setup

1. Install dependencies:
```bash
npm install
```

2. Create a `.env.local` file:
```
NEXT_PUBLIC_BACKEND_URL=http://localhost:3001
NEXT_PUBLIC_WS_URL=ws://localhost:3001/ws
```

3. Start the development server:
```bash
npm run dev
```

## Project Structure

```
frontend/
├── components/        # React components
│   ├── Editor/       # Code editor components
│   ├── Settings/     # Settings panel
│   └── Git/          # Git integration
├── lib/              # Shared utilities
│   ├── hooks/        # Custom React hooks
│   ├── api/          # API integration
│   ├── git/          # Git operations
│   ├── store/        # State management
│   └── websocket/    # WebSocket connection
├── pages/            # Next.js pages
└── styles/           # Global styles
```

## Development

- Run tests: `npm test`
- Build: `npm run build`
- Lint: `npm run lint`

## Production

Build and start the production server:

```bash
npm run build
npm start
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Commit your changes
4. Push to the branch
5. Create a Pull Request