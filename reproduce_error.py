import sys
import subprocess

def main():
    # Try to list available tools in sidecar
    try:
        result = subprocess.run(['cargo', 'run', '--bin', 'webserver', '--', 'tools', 'list'], 
                              cwd='sidecar',
                              capture_output=True,
                              text=True)
        print("Current tools available:")
        print(result.stdout)
        print("\nError output:")
        print(result.stderr)
    except Exception as e:
        print(f"Error running sidecar: {e}")

if __name__ == "__main__":
    main()