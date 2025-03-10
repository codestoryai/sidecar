const { exec } = require('child_process');
const fs = require('fs');
const path = require('path');

console.log('Starting Windows packaging process...');

// Step 1: Build the Next.js application
console.log('Building Next.js application...');
exec('npm run build', (error, stdout, stderr) => {
  if (error) {
    console.error(`Build error: ${error.message}`);
    return;
  }
  if (stderr) {
    console.error(`Build stderr: ${stderr}`);
  }
  console.log(`Build output: ${stdout}`);
  
  // Step 2: Create a simple HTML launcher
  console.log('Creating Windows launcher...');
  const launcherContent = `
<!DOCTYPE html>
<html>
<head>
  <title>Puzzle Game Launcher</title>
  <style>
    body {
      font-family: Arial, sans-serif;
      display: flex;
      justify-content: center;
      align-items: center;
      height: 100vh;
      margin: 0;
      background-color: #f5f5f5;
    }
    .launcher {
      text-align: center;
      padding: 30px;
      background-color: white;
      border-radius: 10px;
      box-shadow: 0 4px 8px rgba(0, 0, 0, 0.1);
    }
    h1 {
      color: #0070f3;
    }
    button {
      background-color: #0070f3;
      color: white;
      border: none;
      padding: 10px 20px;
      font-size: 16px;
      border-radius: 5px;
      cursor: pointer;
      margin-top: 20px;
    }
    button:hover {
      background-color: #0051a8;
    }
  </style>
</head>
<body>
  <div class="launcher">
    <h1>Next.js Sliding Puzzle Game</h1>
    <p>Click the button below to launch the game</p>
    <button onclick="launchGame()">Launch Game</button>
  </div>

  <script>
    function launchGame() {
      // Open the game in the default browser
      window.open('out/index.html', '_blank');
    }
  </script>
</body>
</html>
  `;
  
  fs.writeFileSync('launcher.html', launcherContent);
  console.log('Launcher created: launcher.html');
  
  // Step 3: Create a README for Windows users
  const windowsReadmeContent = `
# Next.js Sliding Puzzle Game for Windows

## How to Run

1. Double-click on "launcher.html" to open the launcher
2. Click the "Launch Game" button to start the game
3. Enjoy playing!

## Troubleshooting

If the game doesn't launch:
- Make sure you have a modern web browser installed (Chrome, Firefox, Edge)
- Check that the "out" folder exists and contains the game files
- If you're still having issues, try opening "out/index.html" directly in your browser

## About

This is a sliding puzzle game built with Next.js. The goal is to arrange the tiles in numerical order.
  `;
  
  fs.writeFileSync('WINDOWS_README.txt', windowsReadmeContent);
  console.log('Windows README created: WINDOWS_README.txt');
  
  console.log('Packaging complete! The game can now be distributed for Windows.');
});