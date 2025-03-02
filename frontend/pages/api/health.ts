import type { NextApiRequest, NextApiResponse } from 'next';
import axios from 'axios';

// This is a simple proxy to the sidecar API health endpoint
export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse
) {
  const { method } = req;

  // Set the sidecar API URL
  const SIDECAR_API_URL = process.env.SIDECAR_API_URL || 'http://localhost:3000/api';

  try {
    if (method === 'GET') {
      // Forward the request to the sidecar API
      const response = await axios.get(`${SIDECAR_API_URL}/health`);
      return res.status(200).json(response.data);
    }

    return res.status(405).json({ error: 'Method not allowed' });
  } catch (error) {
    console.error('API route error:', error);
    
    // If the error is from axios, return the error response
    if (axios.isAxiosError(error) && error.response) {
      return res.status(error.response.status).json(error.response.data);
    }
    
    // If the error is a connection error, return a custom message
    if (axios.isAxiosError(error) && error.code === 'ECONNREFUSED') {
      return res.status(503).json({ 
        error: 'Sidecar API is not available',
        message: 'Make sure the sidecar webserver is running on the configured port'
      });
    }
    
    return res.status(500).json({ error: 'Internal server error' });
  }
}