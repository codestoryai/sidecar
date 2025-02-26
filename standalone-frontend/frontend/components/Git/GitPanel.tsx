import { useState, useEffect } from 'react';
import { useGit } from '../../lib/hooks/useGit';

export function GitPanel() {
  const { status, loading, error, fetchStatus, addFiles, commit } = useGit();
  const [commitMessage, setCommitMessage] = useState('');

  useEffect(() => {
    fetchStatus();
    // Poll for status updates
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  const handleCommit = async () => {
    if (!commitMessage) return;
    await commit(commitMessage);
    setCommitMessage('');
  };

  const handleAddFiles = async (files: string[]) => {
    await addFiles(files);
  };

  return (
    <div className="git-panel p-4 bg-gray-800 text-white">
      <h2 className="text-xl font-bold mb-4">Git Operations</h2>
      
      {error && (
        <div className="bg-red-600 text-white p-2 rounded mb-4">
          {error}
        </div>
      )}

      <div className="mb-6">
        <h3 className="text-lg font-semibold mb-2">Repository Status</h3>
        {loading ? (
          <div className="text-gray-400">Loading...</div>
        ) : (
          <div className="space-y-2">
            {status.staged.length > 0 && (
              <div>
                <h4 className="text-green-400">Staged Files:</h4>
                <ul className="ml-4">
                  {status.staged.map(file => (
                    <li key={file} className="text-sm">{file}</li>
                  ))}
                </ul>
              </div>
            )}
            
            {status.modified.length > 0 && (
              <div>
                <h4 className="text-yellow-400">Modified Files:</h4>
                <ul className="ml-4">
                  {status.modified.map(file => (
                    <li 
                      key={file} 
                      className="text-sm cursor-pointer hover:text-yellow-300"
                      onClick={() => handleAddFiles([file])}
                    >
                      {file}
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {status.untracked.length > 0 && (
              <div>
                <h4 className="text-gray-400">Untracked Files:</h4>
                <ul className="ml-4">
                  {status.untracked.map(file => (
                    <li 
                      key={file} 
                      className="text-sm cursor-pointer hover:text-gray-300"
                      onClick={() => handleAddFiles([file])}
                    >
                      {file}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="commit-section">
        <h3 className="text-lg font-semibold mb-2">Create Commit</h3>
        <div className="space-y-2">
          <textarea
            value={commitMessage}
            onChange={(e) => setCommitMessage(e.target.value)}
            placeholder="Enter commit message..."
            className="w-full p-2 bg-gray-700 rounded border border-gray-600 text-white"
            rows={3}
          />
          <button
            onClick={handleCommit}
            disabled={!commitMessage || loading}
            className={`w-full p-2 rounded font-semibold ${
              !commitMessage || loading
                ? 'bg-gray-600 cursor-not-allowed'
                : 'bg-blue-600 hover:bg-blue-700'
            }`}
          >
            {loading ? 'Committing...' : 'Commit Changes'}
          </button>
        </div>
      </div>
    </div>
  );
}