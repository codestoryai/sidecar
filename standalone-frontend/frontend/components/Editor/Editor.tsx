import { useEffect, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { oneDark } from '@codemirror/theme-one-dark';
import { useSettings } from '../../lib/store/settings';
import { useWebSocket } from '../../lib/hooks/useWebSocket';

export function Editor() {
  const { settings } = useSettings();
  const ws = useWebSocket();
  const [value, setValue] = useState('');
  const [suggestions, setSuggestions] = useState([]);

  useEffect(() => {
    ws.onMessage('completion_response', (payload) => {
      setSuggestions(payload.suggestions);
    });
  }, [ws]);

  const handleChange = async (val: string) => {
    setValue(val);
    ws.send({
      type: 'completion_request',
      payload: {
        code: val,
        cursor: getCursorPosition(),
        language: 'typescript'
      }
    });
  };

  const getCursorPosition = () => {
    // Get cursor position from CodeMirror
    return 0; // Placeholder
  };

  return (
    <div className="h-screen bg-gray-900">
      <CodeMirror
        value={value}
        height="100%"
        theme={oneDark}
        extensions={[javascript()]}
        onChange={handleChange}
        className="text-lg"
      />
      {suggestions.length > 0 && (
        <div className="suggestions-panel absolute right-0 top-0 bg-gray-800 p-4 rounded-lg shadow-lg">
          {suggestions.map((suggestion, i) => (
            <div 
              key={i} 
              className="suggestion-item p-2 hover:bg-gray-700 cursor-pointer"
              onClick={() => setValue(suggestion.text)}
            >
              {suggestion.text}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}