import type { NextApiRequest, NextApiResponse } from 'next';
import axios from 'axios';

// This is a simple proxy to the sidecar API
export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse
) {
  const { method } = req;

  // Set the sidecar API URL
  const SIDECAR_API_URL = process.env.SIDECAR_API_URL || 'http://localhost:3000/api';

  try {
    if (method === 'GET') {
      // Handle GET requests (list files, read file)
      const { directory_path, fs_file_path } = req.query;

      if (directory_path) {
        // List files in directory
        const response = await axios.get(`${SIDECAR_API_URL}/file`, {
          params: { directory_path }
        });
        return res.status(200).json(response.data);
      } else if (fs_file_path) {
        // Read file content
        const response = await axios.get(`${SIDECAR_API_URL}/file`, {
          params: { fs_file_path }
        });
        return res.status(200).json(response.data);
      }

      return res.status(400).json({ error: 'Missing required parameters' });
    } else if (method === 'POST') {
      // Handle POST requests (edit file)
      const { fs_file_path, content } = req.body;

      if (!fs_file_path || content === undefined) {
        return res.status(400).json({ error: 'Missing required parameters' });
      }

      const response = await axios.post(`${SIDECAR_API_URL}/file/edit_file`, {
        fs_file_path,
        content
      });

      return res.status(200).json(response.data);
    }

    return res.status(405).json({ error: 'Method not allowed' });
  } catch (error) {
    console.error('API route error:', error);
    return res.status(500).json({ error: 'Internal server error' });
  }
}