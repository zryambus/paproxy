set shell := ["powershell.exe", "-c"]

release:
    mkdir -Path build -Force
    Get-ChildItem -Path build -File | Remove-Item
    cargo build --release; cp target/release/paproxy.exe ./build
    docker build --output=build .