pathsToZip="sidecar/onnxruntime/ sidecar/qdrant/ sidecar/models/"

# Destination of the zip file
zipFileDestination="sidecar_test.7z"

# Use 7z command to create the archive
7z a -t7z $zipFileDestination $pathsToZip