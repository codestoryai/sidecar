import { Editor } from '../components/Editor/Editor';
import { SettingsPanel } from '../components/Settings/SettingsPanel';
import { GitPanel } from '../components/Git/GitPanel';
import { useEffect } from 'react';
import Head from 'next/head';

export default function Home() {
  useEffect(() => {
    // Set dark mode by default
    document.documentElement.classList.add('dark');
  }, []);

  return (
    <>
      <Head>
        <title>Sidecar IDE</title>
        <meta name="description" content="Standalone Sidecar IDE with AI capabilities" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <main className="flex h-screen bg-gray-900">
        <div className="w-3/4 h-full">
          <Editor />
        </div>
        <div className="w-1/4 h-full overflow-y-auto border-l border-gray-700">
          <SettingsPanel />
          <GitPanel />
        </div>
      </main>
    </>
  );
}