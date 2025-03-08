import { LanguageServerManager } from '@codemirror/lsp';
import { Diagnostic } from 'vscode-languageserver-types';

// Define the LSP client interface
export interface LSPClient {
  connect(): Promise<void>;
  disconnect(): void;
  getDiagnostics(uri: string): Diagnostic[];
  isConnected(): boolean;
}

// Create a class to manage LSP connections
export class LSPManager {
  private static instance: LSPManager;
  private servers: Map<string, LanguageServerManager> = new Map();
  private diagnostics: Map<string, Diagnostic[]> = new Map();

  private constructor() {}

  public static getInstance(): LSPManager {
    if (!LSPManager.instance) {
      LSPManager.instance = new LSPManager();
    }
    return LSPManager.instance;
  }

  // Get or create a language server for a specific language
  public getServer(language: string): LanguageServerManager | null {
    if (this.servers.has(language)) {
      return this.servers.get(language) || null;
    }

    // Create a new language server based on the language
    let server: LanguageServerManager | null = null;
    
    switch (language) {
      case 'javascript':
      case 'typescript':
        server = this.createTypeScriptServer();
        break;
      case 'python':
        server = this.createPythonServer();
        break;
      // Add more language servers as needed
      default:
        return null;
    }

    if (server) {
      this.servers.set(language, server);
    }
    
    return server;
  }

  // Create a TypeScript language server
  private createTypeScriptServer(): LanguageServerManager {
    const serverOptions = {
      serverUri: '/api/lsp/typescript',
      workspaceFolders: [{ name: 'root', uri: 'file:///' }],
      documentSelector: [{ language: 'typescript' }, { language: 'javascript' }],
    };
    
    const server = new LanguageServerManager(serverOptions);
    
    // Listen for diagnostic events
    server.on('diagnostics', (params) => {
      this.diagnostics.set(params.uri, params.diagnostics);
    });
    
    return server;
  }

  // Create a Python language server
  private createPythonServer(): LanguageServerManager {
    const serverOptions = {
      serverUri: '/api/lsp/python',
      workspaceFolders: [{ name: 'root', uri: 'file:///' }],
      documentSelector: [{ language: 'python' }],
    };
    
    const server = new LanguageServerManager(serverOptions);
    
    // Listen for diagnostic events
    server.on('diagnostics', (params) => {
      this.diagnostics.set(params.uri, params.diagnostics);
    });
    
    return server;
  }

  // Get diagnostics for a specific file
  public getDiagnostics(uri: string): Diagnostic[] {
    return this.diagnostics.get(uri) || [];
  }

  // Determine language from file extension
  public static getLanguageFromFilePath(filePath: string): string {
    if (!filePath) return '';
    
    if (filePath.endsWith('.js')) return 'javascript';
    if (filePath.endsWith('.jsx')) return 'javascript';
    if (filePath.endsWith('.ts')) return 'typescript';
    if (filePath.endsWith('.tsx')) return 'typescript';
    if (filePath.endsWith('.py')) return 'python';
    if (filePath.endsWith('.html')) return 'html';
    if (filePath.endsWith('.css')) return 'css';
    if (filePath.endsWith('.json')) return 'json';
    if (filePath.endsWith('.md')) return 'markdown';
    if (filePath.endsWith('.java')) return 'java';
    if (filePath.endsWith('.c')) return 'c';
    if (filePath.endsWith('.cpp')) return 'cpp';
    if (filePath.endsWith('.h')) return 'c';
    if (filePath.endsWith('.hpp')) return 'cpp';
    if (filePath.endsWith('.go')) return 'go';
    if (filePath.endsWith('.rs')) return 'rust';
    if (filePath.endsWith('.rb')) return 'ruby';
    if (filePath.endsWith('.php')) return 'php';
    
    // Default to javascript
    return 'javascript';
  }
}

// Export a singleton instance
export const lspManager = LSPManager.getInstance();