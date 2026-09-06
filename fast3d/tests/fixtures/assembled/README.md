These files are frozen full RDRAM images used as inputs to the interpreter and renderer tests.
They were produced by this repository's text assembler at commit `696a67d`, before it moved to n64.toys.
New or changed inputs are authored in this repo in Rust with `n64-gbi` encoders (see `src/tests/gbi_roundtrip.rs`
and the sm64 corpus builders); these images stay as the byte-level oracle while each one is rewritten that way.
