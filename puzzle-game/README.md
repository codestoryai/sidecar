# Next.js Sliding Puzzle Game

A simple sliding puzzle game built with Next.js for Windows.

## Features

- Three difficulty levels: Easy (3x3), Medium (4x4), and Hard (5x5)
- Timer and move counter to track your progress
- Responsive design that works on all screen sizes
- Optimized for Windows desktop environments

## Getting Started

### Prerequisites

- Node.js 14.0 or later
- npm or yarn

### Installation

1. Clone this repository or download the source code
2. Navigate to the project directory
3. Install dependencies:

```bash
npm install
# or
yarn install
```

### Development

Run the development server:

```bash
npm run dev
# or
yarn dev
```

Open [http://localhost:3000](http://localhost:3000) in your browser to see the game.

### Building for Windows

To create a static build that can be used in a Windows application:

```bash
npm run build
# or
yarn build
```

This will generate a static export in the `out` directory that can be served as a static site or embedded in a Windows application.

## How to Play

1. Select a difficulty level (Easy, Medium, or Hard)
2. Click "Start Game" to begin
3. Click on tiles adjacent to the empty space to move them
4. Arrange the tiles in numerical order to win
5. Try to complete the puzzle in the fewest moves and shortest time!

## License

This project is open source and available under the [MIT License](LICENSE).