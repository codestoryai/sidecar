import { useState, useEffect } from 'react';

interface PuzzleBoardProps {
  difficulty: 'easy' | 'medium' | 'hard';
  onMove: () => void;
  onWin: () => void;
}

const PuzzleBoard: React.FC<PuzzleBoardProps> = ({ difficulty, onMove, onWin }) => {
  // Set grid size based on difficulty
  const gridSize = difficulty === 'easy' ? 3 : difficulty === 'medium' ? 4 : 5;
  
  // State for the puzzle tiles
  const [tiles, setTiles] = useState<number[]>([]);
  const [emptyIndex, setEmptyIndex] = useState<number>(0);
  
  // Initialize the puzzle
  useEffect(() => {
    initializePuzzle();
  }, [difficulty]);
  
  // Check for win condition
  useEffect(() => {
    if (tiles.length === 0) return;
    
    const isWin = checkWinCondition();
    if (isWin) {
      onWin();
    }
  }, [tiles]);
  
  // Initialize the puzzle with shuffled tiles
  const initializePuzzle = () => {
    const totalTiles = gridSize * gridSize;
    const newTiles = Array.from({ length: totalTiles - 1 }, (_, i) => i + 1);
    newTiles.push(0); // Add empty tile (represented by 0)
    
    // Shuffle the tiles (ensuring it's solvable)
    const shuffledTiles = shuffleTiles(newTiles);
    
    setTiles(shuffledTiles);
    setEmptyIndex(shuffledTiles.indexOf(0));
  };
  
  // Shuffle the tiles while ensuring the puzzle is solvable
  const shuffleTiles = (tiles: number[]): number[] => {
    const shuffled = [...tiles];
    let currentIndex = shuffled.length;
    
    // Fisher-Yates shuffle algorithm
    while (currentIndex !== 0) {
      const randomIndex = Math.floor(Math.random() * currentIndex);
      currentIndex--;
      
      [shuffled[currentIndex], shuffled[randomIndex]] = 
      [shuffled[randomIndex], shuffled[currentIndex]];
    }
    
    // Check if the puzzle is solvable
    if (isSolvable(shuffled)) {
      return shuffled;
    } else {
      // If not solvable, swap two tiles to make it solvable
      if (shuffled[0] !== 0 && shuffled[1] !== 0) {
        [shuffled[0], shuffled[1]] = [shuffled[1], shuffled[0]];
      } else {
        [shuffled[shuffled.length - 1], shuffled[shuffled.length - 2]] = 
        [shuffled[shuffled.length - 2], shuffled[shuffled.length - 1]];
      }
      return shuffled;
    }
  };
  
  // Check if the puzzle is solvable
  const isSolvable = (tiles: number[]): boolean => {
    // Count inversions
    let inversions = 0;
    const tilesWithoutEmpty = tiles.filter(tile => tile !== 0);
    
    for (let i = 0; i < tilesWithoutEmpty.length; i++) {
      for (let j = i + 1; j < tilesWithoutEmpty.length; j++) {
        if (tilesWithoutEmpty[i] > tilesWithoutEmpty[j]) {
          inversions++;
        }
      }
    }
    
    // For odd grid sizes, the puzzle is solvable if inversions is even
    if (gridSize % 2 === 1) {
      return inversions % 2 === 0;
    } 
    // For even grid sizes, the puzzle is solvable if:
    // (inversions + row of empty from bottom) is odd
    else {
      const emptyTileIndex = tiles.indexOf(0);
      const emptyTileRow = Math.floor(emptyTileIndex / gridSize);
      const rowFromBottom = gridSize - emptyTileRow;
      return (inversions + rowFromBottom) % 2 === 1;
    }
  };
  
  // Check if the puzzle is solved
  const checkWinCondition = (): boolean => {
    for (let i = 0; i < tiles.length - 1; i++) {
      if (tiles[i] !== i + 1) {
        return false;
      }
    }
    return tiles[tiles.length - 1] === 0;
  };
  
  // Handle tile click
  const handleTileClick = (index: number) => {
    if (!isMovable(index)) return;
    
    const newTiles = [...tiles];
    newTiles[emptyIndex] = newTiles[index];
    newTiles[index] = 0;
    
    setTiles(newTiles);
    setEmptyIndex(index);
    onMove();
  };
  
  // Check if a tile is movable (adjacent to the empty tile)
  const isMovable = (index: number): boolean => {
    // Check if the tile is in the same row and adjacent column
    const sameRow = Math.floor(index / gridSize) === Math.floor(emptyIndex / gridSize);
    const adjacentCol = Math.abs((index % gridSize) - (emptyIndex % gridSize)) === 1;
    
    // Check if the tile is in the same column and adjacent row
    const sameCol = (index % gridSize) === (emptyIndex % gridSize);
    const adjacentRow = Math.abs(Math.floor(index / gridSize) - Math.floor(emptyIndex / gridSize)) === 1;
    
    return (sameRow && adjacentCol) || (sameCol && adjacentRow);
  };
  
  return (
    <div className="puzzle-board">
      {tiles.map((tile, index) => (
        <div
          key={index}
          className={`puzzle-tile ${tile === 0 ? 'empty' : ''} ${isMovable(index) ? 'movable' : ''}`}
          onClick={() => handleTileClick(index)}
        >
          {tile !== 0 && tile}
        </div>
      ))}
      
      <style jsx>{`
        .puzzle-board {
          display: grid;
          grid-template-columns: repeat(${gridSize}, 1fr);
          grid-template-rows: repeat(${gridSize}, 1fr);
          gap: 5px;
          width: min(80vw, 500px);
          height: min(80vw, 500px);
          margin: 0 auto;
        }
        
        .puzzle-tile {
          display: flex;
          align-items: center;
          justify-content: center;
          background-color: white;
          border: 2px solid var(--border-color);
          border-radius: 8px;
          font-size: ${gridSize <= 3 ? '2rem' : gridSize <= 4 ? '1.5rem' : '1.2rem'};
          font-weight: bold;
          cursor: pointer;
          transition: all 0.2s;
          box-shadow: 0 2px 5px rgba(0, 0, 0, 0.1);
        }
        
        .puzzle-tile.empty {
          background-color: transparent;
          border: none;
          box-shadow: none;
          cursor: default;
        }
        
        .puzzle-tile.movable:not(.empty):hover {
          transform: scale(0.95);
          background-color: #e6f7ff;
          border-color: var(--primary-color);
        }
      `}</style>
    </div>
  );
};

export default PuzzleBoard;