import { useState } from 'react';
import { FiSend, FiSettings } from 'react-icons/fi';
import { sendAgentMessage } from '@/services/api';

interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

interface ChatInterfaceProps {
  selectedFile: string | null;
  allFiles: string[];
}

const ChatInterface: React.FC<ChatInterfaceProps> = ({ selectedFile, allFiles }) => {
  const [message, setMessage] = useState<string>('');
  const [chatHistory, setChatHistory] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState<boolean>(false);

  const handleSendMessage = async () => {
    if (!message.trim()) return;

    // Add user message to chat history
    const updatedHistory = [...chatHistory, { role: 'user', content: message }];
    setChatHistory(updatedHistory);
    
    // Clear input
    setMessage('');
    setLoading(true);

    try {
      // Send message to sidecar API
      await sendAgentMessage({
        sessionId: 'frontend-session',
        exchangeId: `exchange-${Date.now()}`,
        editorUrl: window.location.origin,
        query: message,
        userContext: {
          visibleFiles: selectedFile ? [selectedFile] : [],
          openFiles: selectedFile ? [selectedFile] : [],
        },
        repoRef: {
          name: 'local',
          url: '',
        },
        projectLabels: [],
        rootDirectory: '.',
        accessToken: '',
        modelConfiguration: {
          fastModel: 'gpt-3.5-turbo',
          slowModel: 'gpt-4',
        },
        allFiles,
        openFiles: selectedFile ? [selectedFile] : [],
        shell: 'bash',
      });
      
      // In a real implementation, we would handle streaming responses here
      // For now, we'll just add a placeholder response
      setChatHistory([
        ...updatedHistory, 
        { 
          role: 'assistant', 
          content: 'I received your message. In a real implementation, this would be a streaming response from the Sidecar API.' 
        }
      ]);
    } catch (error) {
      console.error('Error sending message:', error);
      setChatHistory([
        ...updatedHistory, 
        { 
          role: 'assistant', 
          content: 'Error: Could not connect to server' 
        }
      ]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="h-full flex flex-col">
      <div className="p-2 border-b border-border flex justify-between items-center">
        <h2 className="text-lg font-semibold">Chat</h2>
        <button className="p-1 hover:bg-gray-800 rounded">
          <FiSettings />
        </button>
      </div>
      
      <div className="flex-grow overflow-auto p-4">
        {chatHistory.map((msg, index) => (
          <div 
            key={index} 
            className={`mb-4 ${msg.role === 'user' ? 'text-right' : 'text-left'}`}
          >
            <div 
              className={`inline-block p-3 rounded-lg ${
                msg.role === 'user' 
                  ? 'bg-primary text-white rounded-tr-none' 
                  : 'bg-gray-800 text-white rounded-tl-none'
              }`}
            >
              {msg.content}
            </div>
          </div>
        ))}
        {loading && (
          <div className="text-left mb-4">
            <div className="inline-block p-3 rounded-lg bg-gray-800 text-white rounded-tl-none">
              Thinking...
            </div>
          </div>
        )}
      </div>
      
      <div className="p-2 border-t border-border">
        <div className="flex">
          <input
            type="text"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
            placeholder="Ask Sidecar..."
            className="flex-grow bg-gray-800 text-white p-2 rounded-l-md focus:outline-none"
            disabled={loading}
          />
          <button 
            onClick={handleSendMessage}
            className={`text-white p-2 rounded-r-md ${
              loading ? 'bg-gray-600 cursor-not-allowed' : 'bg-primary hover:bg-blue-600'
            }`}
            disabled={loading}
          >
            <FiSend />
          </button>
        </div>
      </div>
    </div>
  );
};

export default ChatInterface;