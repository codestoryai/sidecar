# For setting up windows build follow the gist here:
# Add the cargo config and setup the following
# brew install mingw-w64
# https://medium.com/@mr.pankajbahekar/cross-comiple-rust-binaries-on-mac-m1-a252e3a8925e

cargo build --target=x86_64-pc-windows-gnu --verbose --release
zip -r larp target/x86_64-pc-windows-gnu/release/webserver.exe
