import { useState, useEffect } from 'react';
import { FiFolder, FiFile, FiRefreshCw } from 'react-icons/fi';
import { listFiles } from '@/services/api';

interface FileExplorerProps {
  onFileSelect: (file: string) => void;
  selectedFile: string | null;
}

const FileExplorer: React.FC<FileExplorerProps> = ({ onFileSelect, selectedFile }) => {
  const [files, setFiles] = useState<string[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const fetchFiles = async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await listFiles('.');
      const fileList = response.files || [];
      setFiles(fileList);
      
      // Notify parent component about loaded files
      if (onFilesLoaded) {
        onFilesLoaded(fileList);
      }
    } catch (err) {
      console.error('Error fetching files:', err);
      setError('Failed to load files');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchFiles();
  }, []);

  return (
    <div className="h-full flex flex-col">
      <div className="flex justify-between items-center mb-2">
        <h2 className="text-lg font-semibold">Files</h2>
        <button 
          onClick={fetchFiles} 
          className="p-1 hover:bg-gray-800 rounded"
          title="Refresh files"
        >
          <FiRefreshCw className={loading ? 'animate-spin' : ''} />
        </button>
      </div>
      
      {error && (
        <div className="text-red-500 mb-2 text-sm">
          {error}
        </div>
      )}
      
      <div className="overflow-auto flex-grow">
        <ul>
          {files.map((file, index) => (
            <li 
              key={index} 
              className={`flex items-center p-1 cursor-pointer hover:bg-gray-800 rounded ${selectedFile === file ? 'bg-gray-800' : ''}`}
              onClick={() => onFileSelect(file)}
            >
              {file.includes('.') ? <FiFile className="mr-2" /> : <FiFolder className="mr-2" />}
              <span className="truncate">{file}</span>
            </li>
          ))}
          
          {files.length === 0 && !loading && (
            <li className="text-gray-500 text-sm p-1">
              No files found
            </li>
          )}
          
          {loading && files.length === 0 && (
            <li className="text-gray-500 text-sm p-1">
              Loading...
            </li>
          )}
        </ul>
      </div>
    </div>
  );
};

export default FileExplorer;