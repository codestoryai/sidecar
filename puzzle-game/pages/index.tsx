import { useState, useEffect } from 'react';
import Head from 'next/head';
import PuzzleBoard from '../components/PuzzleBoard';
import GameControls from '../components/GameControls';

export default function Home() {
  const [gameStarted, setGameStarted] = useState(false);
  const [difficulty, setDifficulty] = useState<'easy' | 'medium' | 'hard'>('easy');
  const [moves, setMoves] = useState(0);
  const [time, setTime] = useState(0);
  const [isWin, setIsWin] = useState(false);

  // Reset timer when game is won
  useEffect(() => {
    if (isWin) return;
    
    let timer: NodeJS.Timeout;
    if (gameStarted) {
      timer = setInterval(() => {
        setTime(prevTime => prevTime + 1);
      }, 1000);
    }
    
    return () => {
      if (timer) clearInterval(timer);
    };
  }, [gameStarted, isWin]);

  const startGame = () => {
    setGameStarted(true);
    setMoves(0);
    setTime(0);
    setIsWin(false);
  };

  const handleWin = () => {
    setIsWin(true);
    setGameStarted(false);
  };

  const incrementMoves = () => {
    setMoves(prevMoves => prevMoves + 1);
  };

  return (
    <div className="container">
      <Head>
        <title>Sliding Puzzle Game</title>
        <meta name="description" content="A sliding puzzle game built with Next.js" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <h1 className="title">Sliding Puzzle Game</h1>
      
      {!gameStarted && !isWin && (
        <div className="menu">
          <h2 className="subtitle">Select Difficulty</h2>
          <div className="difficulty-buttons">
            <button 
              onClick={() => setDifficulty('easy')} 
              className={difficulty === 'easy' ? 'active' : ''}
            >
              Easy (3x3)
            </button>
            <button 
              onClick={() => setDifficulty('medium')} 
              className={difficulty === 'medium' ? 'active' : ''}
            >
              Medium (4x4)
            </button>
            <button 
              onClick={() => setDifficulty('hard')} 
              className={difficulty === 'hard' ? 'active' : ''}
            >
              Hard (5x5)
            </button>
          </div>
          <button onClick={startGame} className="start-button">Start Game</button>
        </div>
      )}

      {gameStarted && (
        <>
          <GameControls 
            moves={moves} 
            time={time} 
            onReset={startGame} 
          />
          <PuzzleBoard 
            difficulty={difficulty} 
            onMove={incrementMoves} 
            onWin={handleWin} 
          />
        </>
      )}

      {isWin && (
        <div className="win-screen">
          <h2 className="subtitle">Congratulations! You Won!</h2>
          <p className="stats">
            Moves: {moves} | Time: {Math.floor(time / 60)}:{(time % 60).toString().padStart(2, '0')}
          </p>
          <button onClick={startGame} className="start-button">Play Again</button>
        </div>
      )}

      <style jsx>{`
        .menu, .win-screen {
          display: flex;
          flex-direction: column;
          align-items: center;
          gap: 20px;
          margin-bottom: 30px;
        }
        
        .difficulty-buttons {
          display: flex;
          gap: 10px;
          margin-bottom: 20px;
        }
        
        .difficulty-buttons button {
          padding: 10px 20px;
        }
        
        .difficulty-buttons button.active {
          background-color: var(--secondary-color);
        }
        
        .start-button {
          font-size: 1.2rem;
          padding: 12px 24px;
        }
        
        .stats {
          font-size: 1.2rem;
          margin-bottom: 20px;
        }
      `}</style>
    </div>
  );
}