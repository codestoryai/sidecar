import type { NextApiRequest, NextApiResponse } from 'next';
import axios from 'axios';

// This is a proxy to the language server
export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse
) {
  const { method } = req;
  const { language } = req.query;

  // Set the language server URL based on the language
  let languageServerUrl = '';
  
  switch (language) {
    case 'typescript':
      languageServerUrl = process.env.TS_LANGUAGE_SERVER_URL || 'http://localhost:3001';
      break;
    case 'python':
      languageServerUrl = process.env.PYTHON_LANGUAGE_SERVER_URL || 'http://localhost:3002';
      break;
    default:
      return res.status(400).json({ error: `Unsupported language: ${language}` });
  }

  try {
    if (method === 'POST') {
      // Forward the LSP request to the language server
      const response = await axios.post(languageServerUrl, req.body, {
        headers: {
          'Content-Type': 'application/json',
        },
      });

      return res.status(200).json(response.data);
    }

    return res.status(405).json({ error: 'Method not allowed' });
  } catch (error) {
    console.error(`LSP ${language} API route error:`, error);
    
    // If the error is from axios, return the error response
    if (axios.isAxiosError(error) && error.response) {
      return res.status(error.response.status).json(error.response.data);
    }
    
    return res.status(500).json({ error: 'Internal server error' });
  }
}