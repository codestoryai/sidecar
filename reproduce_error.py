import subprocess
import sys

def main():
    # Try to list available tools in zed
    try:
        result = subprocess.run(['cargo', 'run', '--bin', 'larp', '--', 'tools', 'list'], 
                              cwd='larp',
                              capture_output=True,
                              text=True)
        print("Current tools available:")
        print(result.stdout)
        print("\nError output:")
        print(result.stderr)
    except Exception as e:
        print(f"Error running larp: {e}")

if __name__ == "__main__":
    main()
