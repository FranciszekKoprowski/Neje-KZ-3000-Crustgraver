# Crustgraver

Engraving software for the Neje KZ 3000 (possibly also other Neje engravers using a similar protocol) written in Rust.

I made this mostly since i have a Neje KZ 3000 from late 2019 which unfortunatly dosen't have any software that allows me to use it on Linux,
I found the protocol for this software on a abandoned project here: <https://github.com/agressin/pyGraver>
After some tests i managed to make my engraver move using the protocol in this project, unfortunatly the project itself wasn't updated since 2020 and used obsolete functions in pyQt5, i also didn't like that it was entirely made in Python so I decided to make my own software only using the protocol in Rust.

This project was made with help from Anthropic's Claude, since this is the first bigger program I made in Rust.

# Installation

Download a release from the releases tab. For now I'm only providing a binary for Linux since that's the only OS I use.

If you're on Windows or MacOS and want to use this software:

1. Install Rust from <https://rust-lang.org/tools/install/>

2. Clone this repository

3. Enter the Neje-KZ-3000-Crustgraver/crustgraver directory

4. Run `cargo build`

5. Run the compiled binary from Neje-KZ-3000-Crustgraver/crustgraver/target/debug/crustgraver
