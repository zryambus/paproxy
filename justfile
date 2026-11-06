set shell := ["powershell.exe", "-c"]

# Each line runs separately and the recipe stops on the first failure
release:
    mkdir -Path build -Force
    Get-ChildItem -Path build -File | Remove-Item
    cargo build --release --locked --features gui
    cp target/release/paproxy.exe ./build
    docker build --target release --output=build .