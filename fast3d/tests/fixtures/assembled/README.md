These RDRAM inputs were frozen from the text assembler at commit `696a67d`.
New or changed authored inputs use Rust and `n64-gbi` encoders in this repository.

`src/tests/fixtures.rs` records source provenance at that commit, exact `f32` time bits,
texture inputs, and curated-scene membership. The default sample uses time zero and a
32×32 white RGBA8 input; `White(n)` records an n×n input even when a scene does not read it.
Inline-source variants identify their original test function or constant. Time samples have
readable registry names; their retained files use the original hexadecimal time bits.

The pilot builds 27 of the 115 inputs in `src/tests/scene_builders.rs` using
`src/tests/dl_builder.rs`. Their frozen files remain on disk solely for the ignored
`builders_match_frozen_inputs` migration test. It compares the full normalized
`InterpResult`, including final RDP state and diagnostics, with exact equality.
The remaining 88 inputs are still embedded frozen files. Remove the converted files
and the temporary comparator after the batch migration.
