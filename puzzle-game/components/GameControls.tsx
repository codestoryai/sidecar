import React from 'react';

interface GameControlsProps {
  moves: number;
  time: number;
  onReset: () => void;
}

const GameControls: React.FC<GameControlsProps> = ({ moves, time, onReset }) => {
  // Format time as MM:SS
  const formatTime = (seconds: number): string => {
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
  };

  return (
    <div className="game-controls">
      <div className="stats">
        <div className="stat-item">
          <span className="stat-label">Moves:</span>
          <span className="stat-value">{moves}</span>
        </div>
        <div className="stat-item">
          <span className="stat-label">Time:</span>
          <span className="stat-value">{formatTime(time)}</span>
        </div>
      </div>
      <button className="reset-button" onClick={onReset}>
        Reset Game
      </button>

      <style jsx>{`
        .game-controls {
          display: flex;
          flex-direction: column;
          align-items: center;
          margin-bottom: 20px;
          width: 100%;
        }

        .stats {
          display: flex;
          justify-content: center;
          gap: 30px;
          margin-bottom: 15px;
          width: 100%;
        }

        .stat-item {
          display: flex;
          flex-direction: column;
          align-items: center;
        }

        .stat-label {
          font-size: 0.9rem;
          color: #666;
        }

        .stat-value {
          font-size: 1.5rem;
          font-weight: bold;
        }

        .reset-button {
          background-color: var(--secondary-color);
          margin-bottom: 20px;
        }

        .reset-button:hover {
          background-color: #d03167;
        }
      `}</style>
    </div>
  );
};

export default GameControls;