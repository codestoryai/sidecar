import type { NextApiRequest, NextApiResponse } from 'next';
import axios from 'axios';

// This endpoint establishes a connection to the language server
export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse
) {
  const { method } = req;
  const { language, rootPath } = req.body;

  // Set the sidecar API URL
  const SIDECAR_API_URL = process.env.SIDECAR_API_URL || 'http://localhost:3000/api';

  try {
    if (method === 'POST') {
      // Forward the LSP connection request to the sidecar API
      const response = await axios.post(`${SIDECAR_API_URL}/lsp/connect`, {
        language,
        rootPath,
      });

      return res.status(200).json(response.data);
    }

    return res.status(405).json({ error: 'Method not allowed' });
  } catch (error) {
    console.error('LSP connect API route error:', error);
    
    // If the error is from axios, return the error response
    if (axios.isAxiosError(error) && error.response) {
      return res.status(error.response.status).json(error.response.data);
    }
    
    return res.status(500).json({ 
      error: 'Internal server error',
      message: 'Could not connect to language server. Make sure the Sidecar API is running.'
    });
  }
}