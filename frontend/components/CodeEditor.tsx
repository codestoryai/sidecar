import dynamic from 'next/dynamic';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { vscodeDark } from '@uiw/codemirror-theme-vscode';
import { readFile, editFile } from '@/services/api';
import { useEffect, useState } from 'react';

// Dynamically import CodeMirror to avoid SSR issues
const CodeMirror = dynamic(
  () => import('@uiw/react-codemirror').then((mod) => mod.default),
  { ssr: false }
);

interface CodeEditorProps {
  selectedFile: string | null;
}

const CodeEditor: React.FC<CodeEditorProps> = ({ selectedFile }) => {
  const [fileContent, setFileContent] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<boolean>(true);

  useEffect(() => {
    const fetchFileContent = async () => {
      if (!selectedFile) {
        setFileContent('');
        return;
      }

      setLoading(true);
      setError(null);
      try {
        const response = await readFile(selectedFile);
        setFileContent(response.content || '');
        setSaved(true);
      } catch (err) {
        console.error('Error fetching file content:', err);
        setError('Failed to load file content');
      } finally {
        setLoading(false);
      }
    };

    fetchFileContent();
  }, [selectedFile]);

  const handleCodeChange = (value: string) => {
    setFileContent(value);
    setSaved(false);
  };

  const handleSaveFile = async () => {
    if (!selectedFile) return;
    
    setLoading(true);
    setError(null);
    try {
      await editFile(selectedFile, fileContent);
      setSaved(true);
    } catch (err) {
      console.error('Error saving file:', err);
      setError('Failed to save file');
    } finally {
      setLoading(false);
    }
  };

  // Get language support based on file extension
  const getLanguageExtension = () => {
    if (!selectedFile) return javascript();
    
    if (selectedFile.endsWith('.js') || selectedFile.endsWith('.jsx') || 
        selectedFile.endsWith('.ts') || selectedFile.endsWith('.tsx')) {
      return javascript();
    } else if (selectedFile.endsWith('.py')) {
      return python();
    }
    
    return javascript(); // Default to JavaScript
  };

  return (
    <div className="h-full flex flex-col">
      <div className="flex justify-between items-center mb-2">
        <h2 className="text-lg font-semibold truncate">
          {selectedFile || 'No file selected'}
          {!saved && <span className="text-yellow-500 ml-2">*</span>}
        </h2>
        {selectedFile && (
          <button
            onClick={handleSaveFile}
            disabled={loading || saved}
            className={`px-2 py-1 rounded text-sm ${
              saved 
                ? 'bg-gray-700 text-gray-400 cursor-not-allowed' 
                : 'bg-primary text-white hover:bg-blue-600'
            }`}
          >
            {loading ? 'Saving...' : 'Save'}
          </button>
        )}
      </div>
      
      {error && (
        <div className="text-red-500 mb-2 text-sm">
          {error}
        </div>
      )}
      
      <div className="flex-grow">
        {selectedFile ? (
          <CodeMirror
            value={fileContent}
            height="100%"
            theme={vscodeDark}
            extensions={[
              getLanguageExtension(),
              lintGutter(),
              createLSPLinter()
            ]}
            onChange={handleCodeChange}
            ref={editorRef}
            basicSetup={{
              lineNumbers: true,
              highlightActiveLineGutter: true,
              highlightSpecialChars: true,
              foldGutter: true,
              drawSelection: true,
              dropCursor: true,
              allowMultipleSelections: true,
              indentOnInput: true,
              syntaxHighlighting: true,
              bracketMatching: true,
              closeBrackets: true,
              autocompletion: true,
              rectangularSelection: true,
              crosshairCursor: true,
              highlightActiveLine: true,
              highlightSelectionMatches: true,
              closeBracketsKeymap: true,
              searchKeymap: true,
              foldKeymap: true,
              completionKeymap: true,
              lintKeymap: true,
            }}
          />
        ) : (
          <div className="h-full flex items-center justify-center text-gray-500">
            Select a file to edit
          </div>
        )}
      </div>
    </div>
  );
};

export default CodeEditor;