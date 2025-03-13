# Sidecar Frontend

A standalone frontend for the Sidecar AI assistant, built with Next.js, TypeScript, and CodeMirror.

## Features

- File explorer to browse and select files
- Code editor with syntax highlighting for various languages
- Chat interface to interact with Sidecar AI
- Split panel layout for optimal workspace organization

## Prerequisites

- Node.js 18+ and npm/yarn
- Sidecar webserver running locally

## Getting Started

1. Clone this repository
2. Install dependencies:

```bash
npm install
# or
yarn install
```

3. Make sure the Sidecar webserver is running:

```bash
# In the sidecar repository
cargo build --bin webserver
./target/debug/webserver
```

4. Start the development server:

```bash
npm run dev
# or
yarn dev
```

5. Open [http://localhost:3000](http://localhost:3000) in your browser

## Configuration

The frontend is configured to proxy API requests to the Sidecar webserver running on port 3000. If your Sidecar webserver is running on a different port, update the `next.config.js` file:

```javascript
async rewrites() {
  return [
    {
      source: '/api/:path*',
      destination: 'http://localhost:YOUR_PORT/api/:path*', // Change to your port
    },
  ];
}
```

## Usage

1. Browse files in the file explorer panel
2. Click on a file to open it in the code editor
3. Edit files directly in the code editor
4. Use the chat interface to interact with Sidecar AI
5. Ask questions about your code or request assistance with coding tasks

## Building for Production

To build the application for production:

```bash
npm run build
# or
yarn build
```

Then start the production server:

```bash
npm run start
# or
yarn start
```

## License

This project is licensed under the same license as the Sidecar project.