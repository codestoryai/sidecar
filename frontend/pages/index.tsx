import { useState } from 'react';
import Head from 'next/head';
import Split from 'react-split';
import FileExplorer from '@/components/FileExplorer';
import CodeEditor from '@/components/CodeEditor';
import ChatInterface from '@/components/ChatInterface';

export default function Home() {
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);

  const handleFileSelect = (file: string) => {
    setSelectedFile(file);
  };

  return (
    <>
      <Head>
        <title>Sidecar Frontend</title>
        <meta name="description" content="Standalone frontend for Sidecar" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <link rel="icon" href="/favicon.ico" />
      </Head>
      
      <main className="min-h-screen flex flex-col">
        <header className="bg-background border-b border-border p-4">
          <h1 className="text-2xl font-bold text-primary">Sidecar</h1>
        </header>
        
        <div className="flex-grow flex">
          <Split 
            className="split" 
            sizes={[20, 50, 30]} 
            minSize={100} 
            gutterSize={8}
          >
            {/* File Explorer */}
            <div className="bg-background border-r border-border p-2 overflow-auto h-full">
              <FileExplorer 
                onFileSelect={handleFileSelect} 
                selectedFile={selectedFile}
                onFilesLoaded={handleFilesLoaded}
              />
            </div>
            
            {/* Code Editor */}
            <div className="bg-background p-2 h-full">
              <CodeEditor selectedFile={selectedFile} />
            </div>
            
            {/* Chat Interface */}
            <div className="bg-background border-l border-border h-full">
              <ChatInterface 
                selectedFile={selectedFile} 
                allFiles={files} 
              />
            </div>
          </Split>
        </div>
      </main>
    </>
  );
}