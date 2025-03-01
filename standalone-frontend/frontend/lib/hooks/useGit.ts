import { useState, useCallback } from 'react';
import axios from 'axios';

interface GitStatus {
  staged: string[];
  modified: string[];
  untracked: string[];
}

export function useGit() {
  const [status, setStatus] = useState<GitStatus>({
    staged: [],
    modified: [],
    untracked: []
  });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      setLoading(true);
      const response = await axios.get('http://localhost:3001/git/status');
      setStatus(response.data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch git status');
    } finally {
      setLoading(false);
    }
  }, []);

  const addFiles = useCallback(async (files: string[]) => {
    try {
      setLoading(true);
      await axios.post('http://localhost:3001/git/add', { files });
      await fetchStatus();
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to add files');
    } finally {
      setLoading(false);
    }
  }, [fetchStatus]);

  const commit = useCallback(async (message: string) => {
    try {
      setLoading(true);
      await axios.post('http://localhost:3001/git/commit', { message });
      await fetchStatus();
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to commit changes');
    } finally {
      setLoading(false);
    }
  }, [fetchStatus]);

  return {
    status,
    loading,
    error,
    fetchStatus,
    addFiles,
    commit
  };
}