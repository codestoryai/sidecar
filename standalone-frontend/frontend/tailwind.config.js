/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './pages/**/*.{js,ts,jsx,tsx}',
    './components/**/*.{js,ts,jsx,tsx}',
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        'editor-bg': '#1e1e1e',
        'editor-text': '#d4d4d4',
        'editor-line': '#2d2d2d',
      },
    },
  },
  plugins: [],
}