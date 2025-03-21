cargo build --bin webserver --release

pathsToZip="target/release/webserver.exe"

# ❓ how does zip and non-zip compare for windows?

# Destination of the zip file
zipFileDestination="larp.zip"

# Use 7z command to create the archive
7z a -tzip $zipFileDestination $pathsToZip
