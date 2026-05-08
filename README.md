# Crustgraver

Engraving software for the Neje KZ 3000 (possibly also other Neje engravers using a simillar protocol) written in Rust.

I made this modstly since i have a Neje KZ 3000 from late 2019 which unfortunetly dosent have any software that allows me to use it on Linux,
I found the protocol for this software on a abandoned project here: https://github.com/agressin/pyGraver
After some tests i managed to make my engraver move using the protocol in this project, unfortunetly the project itself wasnt updated since 2020 and used obsolete functions in pyQt5, i also didnt like that it was entirely made in Python so I decided to make my own software only usingthe protocol in Rust.

This project was made with help from Anthropic's Claude, since this is the first bigger program I made in Rust.
